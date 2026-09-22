use super::*;
use aipass_crypto::derive_remote_unlock_key;
use zeroize::Zeroizing;

/// A device-local, opt-in alternative unlock envelope. It contains no plaintext key.
/// Keep outside the synced vault; the random access code is displayed only once.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteUnlockEnvelope {
    version: u32,
    vault_id: Uuid,
    password_revision: [u8; 32],
    salt: Uuid,
    wrapped_root_key: Ciphertext,
}

impl RemoteUnlockEnvelope {
    fn aad(&self) -> Result<Vec<u8>, VaultError> {
        Ok(serde_json::to_vec(&(
            "aipass.remote-unlock",
            self.version,
            self.vault_id,
            self.password_revision,
            self.salt,
        ))?)
    }

    fn matches_header(&self, header: &VaultHeader) -> bool {
        self.version == 1
            && self.vault_id == header.vault_id
            && self.password_revision == Vault::header_password_revision(header)
    }

    pub fn matches_vault(&self, vault: &Vault) -> bool {
        self.matches_header(&vault.header)
    }
}

impl Vault {
    pub fn seal_remote_unlock(
        &self,
        code: &SecretString,
    ) -> Result<RemoteUnlockEnvelope, VaultError> {
        self.ensure_sync_ready()?;
        let salt = Uuid::new_v4();
        let key = derive_remote_unlock_key(code, salt.as_bytes())?;
        let mut envelope = RemoteUnlockEnvelope {
            version: 1,
            vault_id: self.vault_id(),
            password_revision: self.password_revision(),
            salt,
            wrapped_root_key: Ciphertext {
                aead: String::new(),
                nonce_b64: String::new(),
                ciphertext_b64: String::new(),
            },
        };
        envelope.wrapped_root_key =
            encrypt_bytes(key.as_bytes(), &envelope.aad()?, self.root_key.as_bytes())?;
        Ok(envelope)
    }

    pub fn open_with_remote_unlock(
        root: impl AsRef<Path>,
        code: &SecretString,
        envelope: &RemoteUnlockEnvelope,
    ) -> Result<Self, VaultError> {
        let root = root.as_ref().to_path_buf();
        let header: VaultHeader = read_json(root.join("manifest.aipmanifest"))?;
        validate_header(&header)?;
        if !envelope.matches_header(&header) {
            return Err(VaultError::UnlockFailed);
        }
        let key = derive_remote_unlock_key(code, envelope.salt.as_bytes())
            .map_err(|_| VaultError::UnlockFailed)?;
        let bytes = Zeroizing::new(
            decrypt_bytes(key.as_bytes(), &envelope.aad()?, &envelope.wrapped_root_key)
                .map_err(|_| VaultError::UnlockFailed)?,
        );
        let mut root_bytes = Zeroizing::new([0_u8; 32]);
        if bytes.len() != root_bytes.len() {
            return Err(VaultError::UnlockFailed);
        }
        root_bytes.copy_from_slice(&bytes);
        let vault = Self::open_with_root_key(
            root,
            header,
            VaultRootKey::from_bytes(*root_bytes),
            VaultError::UnlockFailed,
        )?;
        // Crash recovery can install a newer manifest; check the final password revision too.
        if !envelope.matches_vault(&vault) {
            return Err(VaultError::UnlockFailed);
        }
        Ok(vault)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn create(root: &Path) -> Vault {
        Vault::create_with_device_and_kdf(
            root,
            &SecretString::new("remote-test-password"),
            "test",
            KdfParams::with_random_salt(1024, 1, 1),
        )
        .unwrap()
        .vault
    }

    fn code() -> SecretString {
        SecretString::new(URL_SAFE_NO_PAD.encode([42_u8; 32]))
    }

    #[test]
    fn remote_envelope_unlocks_but_verifier_and_tampered_metadata_do_not() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("vault");
        let vault = create(&root);
        let code = code();
        let envelope = vault.seal_remote_unlock(&code).unwrap();
        let serialized = serde_json::to_string(&envelope).unwrap();
        assert!(!serialized.contains(code.expose()));
        assert!(!serialized.contains("remote-test-password"));
        assert!(!serialized.contains(&STANDARD_NO_PAD.encode(vault.root_key.as_bytes())));
        let unlocked = Vault::open_with_remote_unlock(&root, &code, &envelope).unwrap();
        assert_eq!(unlocked.root_key.as_bytes(), vault.root_key.as_bytes());
        let verifier = Sha256::digest(code.expose().as_bytes());
        let verifier_code = SecretString::new(URL_SAFE_NO_PAD.encode(verifier));
        assert!(Vault::open_with_remote_unlock(&root, &verifier_code, &envelope).is_err());
        assert!(
            Vault::open_with_remote_unlock(&root, &SecretString::new("short"), &envelope).is_err()
        );
        for field in [
            "version",
            "vaultId",
            "passwordRevision",
            "salt",
            "wrappedRootKey",
        ] {
            let mut changed = serde_json::to_value(&envelope).unwrap();
            changed[field] = match field {
                "version" => serde_json::json!(2),
                "vaultId" | "salt" => serde_json::json!(Uuid::new_v4()),
                "passwordRevision" => serde_json::to_value([0_u8; 32]).unwrap(),
                _ => serde_json::to_value(encrypt_bytes(&[0_u8; 32], b"", b"wrong").unwrap())
                    .unwrap(),
            };
            let changed = serde_json::from_value(changed).unwrap();
            assert!(
                Vault::open_with_remote_unlock(&root, &code, &changed).is_err(),
                "{field}"
            );
        }
        let other = temp.path().join("other");
        create(&other);
        assert!(Vault::open_with_remote_unlock(&other, &code, &envelope).is_err());
    }

    #[test]
    fn remote_envelope_rejects_changed_password_and_reloaded_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("vault");
        let mut vault = create(&root);
        let code = code();
        let envelope = vault.seal_remote_unlock(&code).unwrap();
        let mut previous = Vault::open_with_remote_unlock(&root, &code, &envelope).unwrap();
        vault
            .change_master_password_with_kdf(
                &SecretString::new("new-test-password"),
                KdfParams::with_random_salt(1024, 1, 1),
            )
            .unwrap();
        assert!(!envelope.matches_vault(&vault));
        assert!(Vault::open_with_remote_unlock(&root, &code, &envelope).is_err());
        previous.reload_from_disk().unwrap();
        assert!(!envelope.matches_vault(&previous));
        let new_envelope = vault.seal_remote_unlock(&code).unwrap();
        assert!(Vault::open_with_remote_unlock(&root, &code, &new_envelope).is_ok());
    }
}
