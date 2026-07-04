use aipass_crypto::{
    advance_epoch, decrypt_bytes, derive_master_key, derive_recovery_key, encrypt_bytes,
    generate_record_dek, generate_recovery_secret, hmac_fingerprint, mask_secret,
    unwrap_record_dek, wrap_record_dek, Ciphertext, KdfParams, SecretString, VaultEpoch,
    VaultEpochKey, VaultRootKey, WrappedDek, KEY_LEN,
};
use aipass_provider_registry::{
    AuthScheme, GatewayMetadata, InterfaceType, ProviderEndpoint, ProviderEntry, ProviderKind,
    QuotaInfo, SecretRef,
};
use aipass_storage::atomic_write_bytes;
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use zeroize::Zeroize;

const VAULT_FORMAT: &str = "aipass-vault";
const VAULT_VERSION: u16 = 2;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault already exists")]
    AlreadyExists,
    #[error("vault not found")]
    NotFound,
    #[error("vault is locked")]
    Locked,
    #[error("invalid password or corrupted vault")]
    UnlockFailed,
    #[error("invalid recovery key or corrupted vault")]
    RecoveryFailed,
    #[error("unsupported vault format version")]
    UnsupportedVersion,
    #[error("record not found")]
    RecordNotFound,
    #[error("grant not found")]
    GrantNotFound,
    #[error("grant expired or cryptographically erased")]
    GrantExpired,
    #[error("device not found")]
    DeviceNotFound,
    #[error("secret label already exists")]
    DuplicateSecretLabel,
    #[error("cannot remove the last secret from a provider")]
    LastSecret,
    #[error("invalid encrypted vault export")]
    InvalidExport,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] aipass_crypto::CryptoError),
}

impl VaultError {
    fn clone_like(&self) -> Self {
        match self {
            Self::UnlockFailed => Self::UnlockFailed,
            Self::RecoveryFailed => Self::RecoveryFailed,
            Self::UnsupportedVersion => Self::UnsupportedVersion,
            _ => Self::UnlockFailed,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultHeader {
    pub format: String,
    pub version: u16,
    pub vault_id: Uuid,
    pub kdf: KdfParams,
    pub wrapped_root_key: Ciphertext,
    pub recovery_wrapped_root_key: Ciphertext,
    pub wrapped_epoch_key: Ciphertext,
    pub wrapped_index_key: Ciphertext,
    pub current_epoch: VaultEpoch,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEnvelope {
    pub format: String,
    pub version: u16,
    pub vault_id: Uuid,
    pub object_id: Uuid,
    pub object_type: String,
    pub schema_version: u16,
    pub crypto_version: u16,
    pub device_id: Uuid,
    pub lamport: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub wrapped_dek: Option<WrappedDek>,
    pub payload: Option<Ciphertext>,
    pub tombstone: bool,
}

pub type RecordEnvelope = ObjectEnvelope;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub action: String,
    pub record_id: Option<Uuid>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub id: Uuid,
    pub name: String,
    pub trusted: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub first_seen_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
    pub last_epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntryInput {
    pub title: String,
    pub provider_kind: ProviderKind,
    pub provider_id: Option<String>,
    pub domains: Vec<String>,
    pub favicon_url: Option<String>,
    pub endpoints: Vec<ProviderEndpoint>,
    pub interface_type: InterfaceType,
    pub auth_scheme: AuthScheme,
    pub api_key: String,
    #[serde(default)]
    pub secret_label: Option<String>,
    pub default_model: Option<String>,
    #[serde(default)]
    pub model_aliases: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub quota: Option<QuotaInfo>,
    pub gateway: Option<GatewayMetadata>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntryUpdateInput {
    pub title: String,
    pub provider_kind: ProviderKind,
    pub provider_id: Option<String>,
    pub domains: Vec<String>,
    pub favicon_url: Option<String>,
    pub endpoints: Vec<ProviderEndpoint>,
    pub interface_type: InterfaceType,
    pub auth_scheme: AuthScheme,
    pub api_key: Option<String>,
    pub default_model: Option<String>,
    #[serde(default)]
    pub model_aliases: Vec<(String, String)>,
    pub headers: Option<Vec<(String, String)>>,
    pub quota: Option<QuotaInfo>,
    pub gateway: Option<GatewayMetadata>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: Uuid,
    pub title: String,
    pub provider_id: Option<String>,
    pub provider_kind: ProviderKind,
    pub domains: Vec<String>,
    pub favicon_url: Option<String>,
    pub endpoints: Vec<ProviderEndpoint>,
    pub interface_type: InterfaceType,
    pub auth_scheme: AuthScheme,
    pub masked_secret: String,
    pub fingerprint: String,
    pub secret_refs: Vec<SecretRef>,
    pub default_model: Option<String>,
    pub model_aliases: Vec<(String, String)>,
    pub quota: Option<QuotaInfo>,
    pub gateway: Option<GatewayMetadata>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub header_names: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtlGrantSummary {
    pub id: Uuid,
    pub purpose: String,
    pub entry_id: Option<Uuid>,
    pub origin: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedVaultExport {
    pub format: String,
    pub version: u16,
    pub vault_id: Uuid,
    pub kdf: KdfParams,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub payload: Ciphertext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryKit {
    pub recovery_key: String,
}

fn clean_secret_label(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(char::is_control)
        || value.eq_ignore_ascii_case("api key")
        || value.eq_ignore_ascii_case("token")
        || value.eq_ignore_ascii_case("secret")
        || value == "密钥"
        || value == "令牌"
    {
        None
    } else {
        Some(value.to_string())
    }
}

pub struct VaultCreation {
    pub vault: Vault,
    pub recovery_kit: RecoveryKit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultExportPayload {
    files: Vec<VaultExportFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultExportFile {
    relative_path: PathBuf,
    bytes_b64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProviderRecordPlaintext {
    entry: ProviderEntry,
    secrets: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TtlGrantPlaintext {
    id: Uuid,
    purpose: String,
    entry_id: Option<Uuid>,
    field_id: String,
    origin: Option<String>,
    secret: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: OffsetDateTime,
}

pub struct Vault {
    root: PathBuf,
    header: VaultHeader,
    root_key: VaultRootKey,
    epoch_key: VaultEpochKey,
    index_key: [u8; KEY_LEN],
    device_id: Uuid,
}

impl Drop for Vault {
    fn drop(&mut self) {
        self.index_key.zeroize();
    }
}

impl Vault {
    pub fn create(
        root: impl AsRef<Path>,
        password: &SecretString,
    ) -> Result<VaultCreation, VaultError> {
        Self::create_with_device(root, password, "local device")
    }

    pub fn create_with_device(
        root: impl AsRef<Path>,
        password: &SecretString,
        device_name: impl Into<String>,
    ) -> Result<VaultCreation, VaultError> {
        Self::create_with_device_and_kdf(root, password, device_name, KdfParams::interactive())
    }

    fn create_with_device_and_kdf(
        root: impl AsRef<Path>,
        password: &SecretString,
        device_name: impl Into<String>,
        kdf: KdfParams,
    ) -> Result<VaultCreation, VaultError> {
        let root = root.as_ref().to_path_buf();
        if root.join("manifest.aipmanifest").exists() {
            return Err(VaultError::AlreadyExists);
        }
        create_dirs(&root)?;
        let password_key = derive_master_key(password, &kdf)?;
        let root_key = VaultRootKey::generate();
        let recovery_secret = generate_recovery_secret();
        let recovery_key = derive_recovery_key(&SecretString::new(&recovery_secret))?;
        let epoch_key = VaultEpochKey::new(0);
        let index_key = generate_record_dek();
        let now = OffsetDateTime::now_utc();
        let vault_id = Uuid::new_v4();
        let header = VaultHeader {
            format: VAULT_FORMAT.to_string(),
            version: VAULT_VERSION,
            vault_id,
            kdf,
            wrapped_root_key: encrypt_bytes(
                password_key.as_bytes(),
                root_key_aad(vault_id).as_bytes(),
                root_key.as_bytes(),
            )?,
            recovery_wrapped_root_key: encrypt_bytes(
                recovery_key.as_bytes(),
                root_key_aad(vault_id).as_bytes(),
                root_key.as_bytes(),
            )?,
            wrapped_epoch_key: encrypt_bytes(
                root_key.as_bytes(),
                header_key_aad(vault_id, "epoch").as_bytes(),
                epoch_key.as_bytes(),
            )?,
            wrapped_index_key: encrypt_bytes(
                root_key.as_bytes(),
                header_key_aad(vault_id, "index").as_bytes(),
                index_key.as_bytes(),
            )?,
            current_epoch: epoch_key.epoch().clone(),
            created_at: now,
            updated_at: now,
        };
        let mut index_key_bytes = [0_u8; KEY_LEN];
        index_key_bytes.copy_from_slice(index_key.as_bytes());
        write_json(root.join("manifest.aipmanifest"), &header)?;
        let vault = Self {
            root,
            header,
            root_key,
            epoch_key,
            index_key: index_key_bytes,
            device_id: Uuid::new_v4(),
        };
        vault.write_device(DeviceRecord {
            id: vault.device_id,
            name: device_name.into(),
            trusted: true,
            first_seen_at: now,
            last_seen_at: now,
            revoked_at: None,
            last_epoch: 0,
        })?;
        vault.audit("vault.create", None, None)?;
        Ok(VaultCreation {
            vault,
            recovery_kit: RecoveryKit {
                recovery_key: recovery_secret,
            },
        })
    }

    pub fn open(root: impl AsRef<Path>, password: &SecretString) -> Result<Self, VaultError> {
        let root = root.as_ref().to_path_buf();
        let manifest = root.join("manifest.aipmanifest");
        if !manifest.exists() {
            return Err(VaultError::NotFound);
        }
        create_dirs(&root)?;
        let header: VaultHeader = read_json(manifest)?;
        validate_header(&header)?;
        let password_key =
            derive_master_key(password, &header.kdf).map_err(|_| VaultError::UnlockFailed)?;
        let root_key = decrypt_root_key(
            &password_key,
            &header.wrapped_root_key,
            header.vault_id,
            VaultError::UnlockFailed,
        )?;
        Self::open_with_root_key(root, header, root_key, VaultError::UnlockFailed)
    }

    pub fn recover_master_password(
        root: impl AsRef<Path>,
        recovery_key: &SecretString,
        new_password: &SecretString,
    ) -> Result<VaultCreation, VaultError> {
        Self::recover_master_password_with_kdf(
            root,
            recovery_key,
            new_password,
            KdfParams::interactive(),
        )
    }

    fn recover_master_password_with_kdf(
        root: impl AsRef<Path>,
        recovery_secret: &SecretString,
        new_password: &SecretString,
        new_kdf: KdfParams,
    ) -> Result<VaultCreation, VaultError> {
        let root = root.as_ref().to_path_buf();
        let manifest = root.join("manifest.aipmanifest");
        if !manifest.exists() {
            return Err(VaultError::NotFound);
        }
        create_dirs(&root)?;
        let header: VaultHeader = read_json(manifest)?;
        validate_header(&header)?;
        let recovery_key =
            derive_recovery_key(recovery_secret).map_err(|_| VaultError::RecoveryFailed)?;
        let root_key = decrypt_root_key(
            &recovery_key,
            &header.recovery_wrapped_root_key,
            header.vault_id,
            VaultError::RecoveryFailed,
        )?;
        let mut vault =
            Self::open_with_root_key(root, header, root_key, VaultError::RecoveryFailed)?;
        let recovery_kit =
            vault.rewrap_root_key_for_new_password_and_recovery(new_password, new_kdf)?;
        vault.advance_epoch_and_rewrap("vault.recovered")?;
        Ok(VaultCreation {
            vault,
            recovery_kit,
        })
    }

    fn open_with_root_key(
        root: PathBuf,
        header: VaultHeader,
        root_key: VaultRootKey,
        error: VaultError,
    ) -> Result<Self, VaultError> {
        let mut epoch_bytes = decrypt_bytes(
            root_key.as_bytes(),
            header_key_aad(header.vault_id, "epoch").as_bytes(),
            &header.wrapped_epoch_key,
        )
        .map_err(|_| error.clone_like())?;
        let mut index_bytes = decrypt_bytes(
            root_key.as_bytes(),
            header_key_aad(header.vault_id, "index").as_bytes(),
            &header.wrapped_index_key,
        )
        .map_err(|_| error.clone_like())?;
        if epoch_bytes.len() != KEY_LEN || index_bytes.len() != KEY_LEN {
            epoch_bytes.zeroize();
            index_bytes.zeroize();
            return Err(error);
        }
        let mut epoch_key_bytes = [0_u8; KEY_LEN];
        epoch_key_bytes.copy_from_slice(&epoch_bytes);
        let mut index_key = [0_u8; KEY_LEN];
        index_key.copy_from_slice(&index_bytes);
        epoch_bytes.zeroize();
        index_bytes.zeroize();
        Ok(Self {
            root,
            epoch_key: VaultEpochKey::from_parts(header.current_epoch.clone(), epoch_key_bytes),
            header,
            root_key,
            index_key,
            device_id: Uuid::new_v4(),
        })
    }

    pub fn vault_id(&self) -> Uuid {
        self.header.vault_id
    }

    pub fn current_device_id(&self) -> Uuid {
        self.device_id
    }

    pub fn current_epoch(&self) -> VaultEpoch {
        self.header.current_epoch.clone()
    }

    pub fn config_backup_key(&self) -> [u8; KEY_LEN] {
        let mut hasher = Sha256::new();
        hasher.update(self.index_key);
        hasher.update(self.header.vault_id.as_bytes());
        hasher.update(b"aipass config backup key v1");
        let digest = hasher.finalize();
        let mut key = [0_u8; KEY_LEN];
        key.copy_from_slice(&digest[..KEY_LEN]);
        key
    }

    pub fn fingerprint_secret(&self, secret: &str) -> String {
        hmac_fingerprint(&self.index_key, secret)
    }

    pub fn change_master_password(
        &mut self,
        new_password: &SecretString,
    ) -> Result<(), VaultError> {
        self.change_master_password_with_kdf(new_password, KdfParams::interactive())
    }

    fn change_master_password_with_kdf(
        &mut self,
        new_password: &SecretString,
        new_kdf: KdfParams,
    ) -> Result<(), VaultError> {
        self.rewrap_root_key_for_new_password(new_password, new_kdf)?;
        self.advance_epoch_and_rewrap("vault.password_changed")?;
        Ok(())
    }

    fn rewrap_root_key_for_new_password(
        &mut self,
        new_password: &SecretString,
        new_kdf: KdfParams,
    ) -> Result<(), VaultError> {
        let new_password_key = derive_master_key(new_password, &new_kdf)?;
        self.header.kdf = new_kdf;
        self.header.wrapped_root_key = encrypt_bytes(
            new_password_key.as_bytes(),
            root_key_aad(self.header.vault_id).as_bytes(),
            self.root_key.as_bytes(),
        )?;
        Ok(())
    }

    fn rewrap_root_key_for_new_password_and_recovery(
        &mut self,
        new_password: &SecretString,
        new_kdf: KdfParams,
    ) -> Result<RecoveryKit, VaultError> {
        self.rewrap_root_key_for_new_password(new_password, new_kdf)?;
        let recovery_secret = generate_recovery_secret();
        let recovery_key = derive_recovery_key(&SecretString::new(&recovery_secret))?;
        self.header.recovery_wrapped_root_key = encrypt_bytes(
            recovery_key.as_bytes(),
            root_key_aad(self.header.vault_id).as_bytes(),
            self.root_key.as_bytes(),
        )?;
        Ok(RecoveryKit {
            recovery_key: recovery_secret,
        })
    }

    pub fn advance_epoch_and_rewrap(&mut self, reason: &str) -> Result<VaultEpoch, VaultError> {
        let old_epoch = self.epoch_key.clone();
        let next_epoch = advance_epoch(&old_epoch)?;
        let paths = self.encrypted_object_paths()?;
        for path in paths {
            self.reencrypt_envelope_with_new_epoch(&path, &old_epoch, &next_epoch)?;
        }
        self.epoch_key = next_epoch;
        self.header.current_epoch = self.epoch_key.epoch().clone();
        self.rewrap_header_keys()?;
        self.audit(reason, None, Some("epoch advanced"))?;
        Ok(self.header.current_epoch.clone())
    }

    pub fn add_provider(&self, input: ProviderEntryInput) -> Result<Uuid, VaultError> {
        let now = OffsetDateTime::now_utc();
        let id = Uuid::new_v4();
        let secret_id = Uuid::new_v4().to_string();
        let fingerprint = hmac_fingerprint(&self.index_key, &input.api_key);
        let secret_label = clean_secret_label(input.secret_label.as_deref())
            .unwrap_or_else(|| "primary".to_string());
        let entry = ProviderEntry {
            id,
            title: input.title,
            provider_kind: input.provider_kind,
            provider_id: input.provider_id,
            domains: input.domains,
            favicon_url: input.favicon_url,
            endpoints: input.endpoints,
            interface_type: input.interface_type,
            auth_scheme: input.auth_scheme,
            secret_refs: vec![SecretRef {
                id: secret_id.clone(),
                label: secret_label,
                masked: mask_secret(&input.api_key),
                fingerprint,
            }],
            default_model: input.default_model,
            model_aliases: input.model_aliases,
            headers: input.headers,
            quota: input.quota,
            gateway: input.gateway,
            tags: input.tags,
            notes: input.notes,
            created_at: now,
            updated_at: now,
            last_used_at: None,
            archived_at: None,
            deleted_at: None,
        };
        let mut secrets = BTreeMap::new();
        secrets.insert(secret_id, input.api_key);
        self.write_provider_record(id, &ProviderRecordPlaintext { entry, secrets })?;
        self.audit("provider.create", Some(id), None)?;
        Ok(id)
    }

    pub fn add_secret(
        &self,
        id: Uuid,
        label: impl Into<String>,
        secret: impl Into<String>,
    ) -> Result<String, VaultError> {
        let label = label.into();
        let secret = secret.into();
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        if plaintext
            .entry
            .secret_refs
            .iter()
            .any(|existing| existing.label == label)
        {
            return Err(VaultError::DuplicateSecretLabel);
        }
        let secret_id = Uuid::new_v4().to_string();
        plaintext.entry.secret_refs.push(SecretRef {
            id: secret_id.clone(),
            label,
            masked: mask_secret(&secret),
            fingerprint: hmac_fingerprint(&self.index_key, &secret),
        });
        plaintext.entry.updated_at = OffsetDateTime::now_utc();
        plaintext.secrets.insert(secret_id.clone(), secret);
        self.write_provider_record(id, &plaintext)?;
        self.audit("secret.add", Some(id), None)?;
        Ok(secret_id)
    }

    pub fn remove_secret(&self, id: Uuid, label_or_id: &str) -> Result<(), VaultError> {
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        if plaintext.entry.secret_refs.len() <= 1 {
            return Err(VaultError::LastSecret);
        }
        let before = plaintext.entry.secret_refs.len();
        let removed = plaintext
            .entry
            .secret_refs
            .iter()
            .find(|secret| secret.id == label_or_id || secret.label == label_or_id)
            .map(|secret| secret.id.clone())
            .ok_or(VaultError::RecordNotFound)?;
        plaintext
            .entry
            .secret_refs
            .retain(|secret| secret.id != removed);
        plaintext.secrets.remove(&removed);
        if plaintext.entry.secret_refs.len() == before {
            return Err(VaultError::RecordNotFound);
        }
        plaintext.entry.updated_at = OffsetDateTime::now_utc();
        self.write_provider_record(id, &plaintext)?;
        self.audit("secret.remove", Some(id), None)?;
        Ok(())
    }

    pub fn update_provider(
        &self,
        id: Uuid,
        input: ProviderEntryUpdateInput,
    ) -> Result<(), VaultError> {
        let path = self.record_path(id);
        let old = self.decrypt_provider_path(&path)?;
        let now = OffsetDateTime::now_utc();
        let secret_id = old
            .entry
            .secret_refs
            .first()
            .map(|secret| secret.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let old_secret = old.secrets.get(&secret_id).cloned();
        let api_key = input
            .api_key
            .clone()
            .or(old_secret)
            .ok_or(VaultError::RecordNotFound)?;
        let fingerprint = hmac_fingerprint(&self.index_key, &api_key);
        let mut secret_refs = old.entry.secret_refs;
        if secret_refs.is_empty() {
            secret_refs.push(SecretRef {
                id: secret_id.clone(),
                label: "primary".to_string(),
                masked: mask_secret(&api_key),
                fingerprint,
            });
        } else if let Some(primary) = secret_refs.first_mut() {
            primary.masked = mask_secret(&api_key);
            primary.fingerprint = fingerprint;
        }
        let entry = ProviderEntry {
            id,
            title: input.title,
            provider_kind: input.provider_kind,
            provider_id: input.provider_id,
            domains: input.domains,
            favicon_url: input.favicon_url,
            endpoints: input.endpoints,
            interface_type: input.interface_type,
            auth_scheme: input.auth_scheme,
            secret_refs,
            default_model: input.default_model,
            model_aliases: input.model_aliases,
            headers: input.headers.unwrap_or(old.entry.headers),
            quota: input.quota,
            gateway: input.gateway,
            tags: input.tags,
            notes: input.notes,
            created_at: old.entry.created_at,
            updated_at: now,
            last_used_at: old.entry.last_used_at,
            archived_at: old.entry.archived_at,
            deleted_at: old.entry.deleted_at,
        };
        let mut secrets = old.secrets;
        secrets.insert(secret_id, api_key);
        self.write_provider_record(id, &ProviderRecordPlaintext { entry, secrets })?;
        self.audit("provider.update", Some(id), None)?;
        Ok(())
    }

    pub fn set_provider_favicon_url(
        &self,
        id: Uuid,
        favicon_url: impl Into<String>,
    ) -> Result<Option<EntrySummary>, VaultError> {
        let favicon_url = favicon_url.into();
        let favicon_url = favicon_url.trim();
        if favicon_url.is_empty() {
            return Ok(None);
        }
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        if plaintext
            .entry
            .favicon_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
        {
            return Ok(None);
        }
        plaintext.entry.favicon_url = Some(favicon_url.to_string());
        plaintext.entry.updated_at = OffsetDateTime::now_utc();
        self.write_provider_record(id, &plaintext)?;
        self.audit("provider.favicon_backfill", Some(id), None)?;
        Ok(Some(summary_from_plaintext(&plaintext)))
    }

    pub fn get_provider_summary(&self, id: Uuid) -> Result<EntrySummary, VaultError> {
        let path = self.record_path(id);
        let plaintext = self.decrypt_provider_path(&path)?;
        Ok(summary_from_plaintext(&plaintext))
    }

    pub fn get_provider_summary_from_path(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<EntrySummary, VaultError> {
        let plaintext = self.decrypt_provider_path(path.as_ref())?;
        Ok(summary_from_plaintext(&plaintext))
    }

    pub fn list_provider_summaries(&self) -> Result<Vec<EntrySummary>, VaultError> {
        let mut summaries = Vec::new();
        for envelope_path in self.record_paths()? {
            let plaintext = self.decrypt_provider_path(&envelope_path)?;
            if plaintext.entry.deleted_at.is_some() {
                continue;
            }
            if plaintext.entry.archived_at.is_none() {
                summaries.push(summary_from_plaintext(&plaintext));
            }
        }
        summaries.sort_by_key(|entry| entry.title.to_lowercase());
        Ok(summaries)
    }

    pub fn list_archived_provider_summaries(&self) -> Result<Vec<EntrySummary>, VaultError> {
        let mut summaries = Vec::new();
        for envelope_path in self.record_paths()? {
            let plaintext = self.decrypt_provider_path(&envelope_path)?;
            if plaintext.entry.deleted_at.is_some() {
                continue;
            }
            if plaintext.entry.archived_at.is_some() {
                summaries.push(summary_from_plaintext(&plaintext));
            }
        }
        summaries.sort_by_key(|entry| entry.title.to_lowercase());
        Ok(summaries)
    }

    pub fn list_trash_provider_summaries(&self) -> Result<Vec<EntrySummary>, VaultError> {
        let mut summaries = Vec::new();
        for envelope_path in self.record_paths()? {
            let plaintext = self.decrypt_provider_path(&envelope_path)?;
            if plaintext.entry.deleted_at.is_some() {
                summaries.push(summary_from_plaintext(&plaintext));
            }
        }
        summaries.sort_by_key(|entry| entry.deleted_at.unwrap_or(OffsetDateTime::UNIX_EPOCH));
        summaries.reverse();
        Ok(summaries)
    }

    pub fn search(&self, query: &str) -> Result<Vec<EntrySummary>, VaultError> {
        let q = query.trim().to_lowercase();
        let candidate_fingerprint = if looks_like_secret(query) || query.len() >= 20 {
            Some(hmac_fingerprint(&self.index_key, query.trim()))
        } else {
            None
        };
        let mut matches = Vec::new();
        for envelope_path in self.record_paths()? {
            let plaintext = self.decrypt_provider_path(&envelope_path)?;
            if plaintext.entry.deleted_at.is_some() || plaintext.entry.archived_at.is_some() {
                continue;
            }
            if plaintext_matches_query(&plaintext, &q, candidate_fingerprint.as_deref()) {
                matches.push(summary_from_plaintext(&plaintext));
            }
        }
        matches.sort_by_key(|entry| entry.title.to_lowercase());
        Ok(matches)
    }

    pub fn lookup_by_origin(&self, origin_or_url: &str) -> Result<Vec<EntrySummary>, VaultError> {
        let host = host_from_origin(origin_or_url);
        Ok(self
            .list_provider_summaries()?
            .into_iter()
            .filter(|entry| {
                entry.domains.iter().any(|domain| {
                    host == domain.to_lowercase()
                        || host.ends_with(&format!(".{}", domain.to_lowercase()))
                }) || entry
                    .endpoints
                    .iter()
                    .filter_map(|endpoint| endpoint.url.as_ref())
                    .any(|url| host_from_origin(url) == host)
            })
            .collect())
    }

    pub fn reveal_secret(&self, id: Uuid) -> Result<String, VaultError> {
        self.reveal_secret_field(id, "primary")
    }

    pub fn reveal_secret_field(&self, id: Uuid, label_or_id: &str) -> Result<String, VaultError> {
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        let secret_id = if label_or_id == "primary" {
            plaintext
                .entry
                .secret_refs
                .first()
                .map(|secret| secret.id.clone())
        } else {
            plaintext
                .entry
                .secret_refs
                .iter()
                .find(|secret| secret.id == label_or_id || secret.label == label_or_id)
                .map(|secret| secret.id.clone())
        }
        .ok_or(VaultError::RecordNotFound)?;
        let secret = plaintext
            .secrets
            .get(&secret_id)
            .cloned()
            .ok_or(VaultError::RecordNotFound)?;
        plaintext.entry.last_used_at = Some(OffsetDateTime::now_utc());
        self.write_provider_record(id, &plaintext)?;
        self.audit("secret.reveal", Some(id), None)?;
        Ok(secret)
    }

    pub fn create_secret_grant(
        &self,
        entry_id: Uuid,
        purpose: impl Into<String>,
        ttl_seconds: i64,
        origin: Option<String>,
    ) -> Result<TtlGrantSummary, VaultError> {
        let purpose = purpose.into();
        let secret = self.reveal_secret(entry_id)?;
        let now = OffsetDateTime::now_utc();
        let expires_at = now + Duration::seconds(ttl_seconds.max(1));
        let id = Uuid::new_v4();
        let grant = TtlGrantPlaintext {
            id,
            purpose: purpose.clone(),
            entry_id: Some(entry_id),
            field_id: "primary".to_string(),
            origin: origin.clone(),
            secret,
            created_at: now,
            expires_at,
        };
        self.write_envelope(
            self.grant_path(id),
            id,
            "ttl_grant",
            1,
            &serde_json::to_vec(&grant)?,
        )?;
        self.audit("grant.create", Some(entry_id), Some(&purpose))?;
        Ok(TtlGrantSummary {
            id,
            purpose,
            entry_id: Some(entry_id),
            origin,
            expires_at,
        })
    }

    pub fn consume_secret_grant(&self, grant_id: Uuid) -> Result<String, VaultError> {
        let path = self.grant_path(grant_id);
        if !path.exists() {
            return Err(VaultError::GrantNotFound);
        }
        let grant: TtlGrantPlaintext =
            self.decrypt_envelope_path(&path).map_err(|err| match err {
                VaultError::Crypto(_) => VaultError::GrantExpired,
                other => other,
            })?;
        if OffsetDateTime::now_utc() >= grant.expires_at {
            self.expire_grant(grant_id)?;
            return Err(VaultError::GrantExpired);
        }
        let grant_entry_id = grant.entry_id;
        if let Some(entry_id) = grant_entry_id {
            let _ = self.touch_provider_last_used(entry_id);
        }
        self.audit("grant.consume", grant_entry_id, Some(&grant.purpose))?;
        Ok(grant.secret)
    }

    pub fn expire_grant(&self, grant_id: Uuid) -> Result<(), VaultError> {
        let path = self.grant_path(grant_id);
        if !path.exists() {
            return Err(VaultError::GrantNotFound);
        }
        let mut envelope: ObjectEnvelope = read_json(&path)?;
        envelope.wrapped_dek = None;
        envelope.tombstone = true;
        envelope.updated_at = OffsetDateTime::now_utc();
        envelope.lamport = envelope.lamport.saturating_add(1);
        write_json(path, &envelope)?;
        self.audit("grant.expire", None, Some(&grant_id.to_string()))?;
        Ok(())
    }

    pub fn archive_provider(&self, id: Uuid) -> Result<(), VaultError> {
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        plaintext.entry.archived_at = Some(OffsetDateTime::now_utc());
        plaintext.entry.updated_at = OffsetDateTime::now_utc();
        self.write_provider_record(id, &plaintext)?;
        self.audit("provider.archive", Some(id), None)?;
        Ok(())
    }

    pub fn restore_provider(&self, id: Uuid) -> Result<(), VaultError> {
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        plaintext.entry.archived_at = None;
        plaintext.entry.deleted_at = None;
        plaintext.entry.updated_at = OffsetDateTime::now_utc();
        self.write_provider_record(id, &plaintext)?;
        self.audit("provider.restore", Some(id), None)?;
        Ok(())
    }

    pub fn trash_provider(&self, id: Uuid) -> Result<(), VaultError> {
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        plaintext.entry.deleted_at = Some(OffsetDateTime::now_utc());
        plaintext.entry.updated_at = OffsetDateTime::now_utc();
        self.write_provider_record(id, &plaintext)?;
        self.audit("provider.trash", Some(id), None)?;
        Ok(())
    }

    pub fn purge_expired_trash(&self, ttl: time::Duration) -> Result<usize, VaultError> {
        let cutoff = OffsetDateTime::now_utc() - ttl;
        let mut purged = 0;
        for envelope_path in self.record_paths()? {
            let plaintext = self.decrypt_provider_path(&envelope_path)?;
            if let Some(deleted_at) = plaintext.entry.deleted_at {
                if deleted_at < cutoff {
                    self.delete_provider_permanently(plaintext.entry.id)?;
                    purged += 1;
                }
            }
        }
        Ok(purged)
    }

    pub fn delete_provider_permanently(&self, id: Uuid) -> Result<(), VaultError> {
        let path = self.record_path(id);
        if !path.exists() {
            return Err(VaultError::RecordNotFound);
        }
        fs::remove_file(path)?;
        self.audit("provider.delete", Some(id), None)?;
        Ok(())
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceRecord>, VaultError> {
        let mut devices: Vec<DeviceRecord> = Vec::new();
        for path in encrypted_paths(&self.root.join("devices"), "aipdevice")? {
            devices.push(self.decrypt_envelope_path(&path)?);
        }
        devices.sort_by_key(|device| device.first_seen_at);
        Ok(devices)
    }

    pub fn revoke_device(&mut self, device_id: Uuid) -> Result<(), VaultError> {
        let path = self.device_path(device_id);
        if !path.exists() {
            return Err(VaultError::DeviceNotFound);
        }
        let mut device: DeviceRecord = self.decrypt_envelope_path(&path)?;
        device.trusted = false;
        device.revoked_at = Some(OffsetDateTime::now_utc());
        device.last_epoch = self.header.current_epoch.epoch + 1;
        self.write_device(device)?;
        self.advance_epoch_and_rewrap("device.revoke")?;
        Ok(())
    }

    pub fn export_encrypted(
        &self,
        export_password: &SecretString,
    ) -> Result<EncryptedVaultExport, VaultError> {
        self.export_encrypted_with_kdf(export_password, KdfParams::interactive())
    }

    fn export_encrypted_with_kdf(
        &self,
        export_password: &SecretString,
        kdf: KdfParams,
    ) -> Result<EncryptedVaultExport, VaultError> {
        let export_key = derive_master_key(export_password, &kdf)?;
        let files = exportable_files(&self.root)?
            .into_iter()
            .map(|relative_path| {
                let bytes = fs::read(self.root.join(&relative_path))?;
                Ok(VaultExportFile {
                    relative_path,
                    bytes_b64: STANDARD_NO_PAD.encode(bytes),
                })
            })
            .collect::<Result<Vec<_>, VaultError>>()?;
        let payload = VaultExportPayload { files };
        let created_at = OffsetDateTime::now_utc();
        Ok(EncryptedVaultExport {
            format: "aipass-encrypted-vault-export".to_string(),
            version: 1,
            vault_id: self.header.vault_id,
            kdf,
            created_at,
            payload: encrypt_bytes(
                export_key.as_bytes(),
                export_aad(self.header.vault_id, created_at).as_bytes(),
                &serde_json::to_vec(&payload)?,
            )?,
        })
    }

    pub fn import_encrypted(
        root: impl AsRef<Path>,
        export_password: &SecretString,
        export: &EncryptedVaultExport,
    ) -> Result<(), VaultError> {
        let root = root.as_ref();
        if root.join("manifest.aipmanifest").exists() {
            return Err(VaultError::AlreadyExists);
        }
        let export_key = derive_master_key(export_password, &export.kdf)?;
        let bytes = decrypt_bytes(
            export_key.as_bytes(),
            export_aad(export.vault_id, export.created_at).as_bytes(),
            &export.payload,
        )?;
        let payload: VaultExportPayload = serde_json::from_slice(&bytes)?;
        for file in payload.files {
            let relative_path = checked_relative_path(&file.relative_path)?;
            let bytes = STANDARD_NO_PAD
                .decode(file.bytes_b64.as_bytes())
                .map_err(|_| VaultError::InvalidExport)?;
            let path = root.join(relative_path);
            atomic_write_bytes(&path, &bytes)?;
        }
        create_dirs(root)?;
        Ok(())
    }

    fn write_provider_record(
        &self,
        id: Uuid,
        plaintext: &ProviderRecordPlaintext,
    ) -> Result<(), VaultError> {
        self.write_envelope(
            self.record_path(id),
            id,
            "provider_entry",
            1,
            &serde_json::to_vec(plaintext)?,
        )
    }

    fn touch_provider_last_used(&self, id: Uuid) -> Result<(), VaultError> {
        let path = self.record_path(id);
        let mut plaintext = self.decrypt_provider_path(&path)?;
        plaintext.entry.last_used_at = Some(OffsetDateTime::now_utc());
        self.write_provider_record(id, &plaintext)
    }

    fn write_device(&self, device: DeviceRecord) -> Result<(), VaultError> {
        self.write_envelope(
            self.device_path(device.id),
            device.id,
            "device_record",
            1,
            &serde_json::to_vec(&device)?,
        )
    }

    fn write_envelope(
        &self,
        path: PathBuf,
        object_id: Uuid,
        object_type: &str,
        schema_version: u16,
        plaintext: &[u8],
    ) -> Result<(), VaultError> {
        let updated_at = OffsetDateTime::now_utc();
        let lamport = updated_at.unix_timestamp_nanos() as u64;
        let mut envelope = ObjectEnvelope {
            format: "aipass-object".to_string(),
            version: 1,
            vault_id: self.header.vault_id,
            object_id,
            object_type: object_type.to_string(),
            schema_version,
            crypto_version: 1,
            device_id: self.device_id,
            lamport,
            updated_at,
            wrapped_dek: None,
            payload: None,
            tombstone: false,
        };
        let aad = object_aad(&envelope);
        let record_dek = generate_record_dek();
        envelope.payload = Some(encrypt_bytes(
            record_dek.as_bytes(),
            aad.as_bytes(),
            plaintext,
        )?);
        envelope.wrapped_dek = Some(wrap_record_dek(
            &self.epoch_key,
            &record_dek,
            aad.as_bytes(),
        )?);
        write_json(path, &envelope)?;
        Ok(())
    }

    fn decrypt_provider_path(&self, path: &Path) -> Result<ProviderRecordPlaintext, VaultError> {
        if !path.exists() {
            return Err(VaultError::RecordNotFound);
        }
        self.decrypt_envelope_path(path)
    }

    fn decrypt_envelope_path<T: for<'de> Deserialize<'de>>(
        &self,
        path: &Path,
    ) -> Result<T, VaultError> {
        let envelope: ObjectEnvelope = read_json(path)?;
        let bytes = self.decrypt_envelope_bytes(&envelope)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn decrypt_envelope_bytes(&self, envelope: &ObjectEnvelope) -> Result<Vec<u8>, VaultError> {
        decrypt_envelope_with_epoch(envelope, &self.epoch_key)
    }

    fn reencrypt_envelope_with_new_epoch(
        &self,
        path: &Path,
        old_epoch: &VaultEpochKey,
        new_epoch: &VaultEpochKey,
    ) -> Result<(), VaultError> {
        let mut envelope: ObjectEnvelope = read_json(path)?;
        if envelope.tombstone || envelope.wrapped_dek.is_none() || envelope.payload.is_none() {
            return Ok(());
        }
        let plaintext = decrypt_envelope_with_epoch(&envelope, old_epoch)?;
        envelope.device_id = self.device_id;
        envelope.updated_at = OffsetDateTime::now_utc();
        envelope.lamport = envelope.lamport.saturating_add(1);
        let aad = object_aad(&envelope);
        let record_dek = generate_record_dek();
        envelope.payload = Some(encrypt_bytes(
            record_dek.as_bytes(),
            aad.as_bytes(),
            &plaintext,
        )?);
        envelope.wrapped_dek = Some(wrap_record_dek(new_epoch, &record_dek, aad.as_bytes())?);
        write_json(path, &envelope)?;
        Ok(())
    }

    fn encrypted_object_paths(&self) -> Result<Vec<PathBuf>, VaultError> {
        let mut paths = Vec::new();
        paths.extend(encrypted_paths(&self.root.join("objects"), "aipobj")?);
        paths.extend(encrypted_paths(&self.root.join("devices"), "aipdevice")?);
        paths.extend(encrypted_paths(&self.root.join("grants"), "aipgrant")?);
        paths.extend(encrypted_paths(&self.root.join("audit"), "aipaudit")?);
        Ok(paths)
    }

    fn record_path(&self, id: Uuid) -> PathBuf {
        self.root.join("objects").join(format!("{id}.aipobj"))
    }

    fn grant_path(&self, id: Uuid) -> PathBuf {
        self.root.join("grants").join(format!("{id}.aipgrant"))
    }

    fn device_path(&self, id: Uuid) -> PathBuf {
        self.root.join("devices").join(format!("{id}.aipdevice"))
    }

    fn record_paths(&self) -> Result<Vec<PathBuf>, VaultError> {
        encrypted_paths(&self.root.join("objects"), "aipobj")
    }

    fn audit(
        &self,
        action: &str,
        record_id: Option<Uuid>,
        detail: Option<&str>,
    ) -> Result<(), VaultError> {
        let event = AuditEvent {
            id: Uuid::new_v4(),
            at: OffsetDateTime::now_utc(),
            action: action.to_string(),
            record_id,
            detail: detail.map(redact_detail),
        };
        self.write_envelope(
            self.root
                .join("audit")
                .join(format!("{}.aipaudit", event.id)),
            event.id,
            "audit_event",
            1,
            &serde_json::to_vec(&event)?,
        )
    }

    fn rewrap_header_keys(&mut self) -> Result<(), VaultError> {
        self.header.wrapped_epoch_key = encrypt_bytes(
            self.root_key.as_bytes(),
            header_key_aad(self.header.vault_id, "epoch").as_bytes(),
            self.epoch_key.as_bytes(),
        )?;
        self.header.wrapped_index_key = encrypt_bytes(
            self.root_key.as_bytes(),
            header_key_aad(self.header.vault_id, "index").as_bytes(),
            &self.index_key,
        )?;
        self.header.current_epoch = self.epoch_key.epoch().clone();
        self.header.updated_at = OffsetDateTime::now_utc();
        write_json(self.root.join("manifest.aipmanifest"), &self.header)?;
        Ok(())
    }

    #[cfg(test)]
    fn decrypt_envelope_with_epoch_for_test(
        &self,
        id: Uuid,
        epoch: &VaultEpochKey,
    ) -> Result<Vec<u8>, VaultError> {
        let envelope: ObjectEnvelope = read_json(self.record_path(id))?;
        decrypt_envelope_with_epoch(&envelope, epoch)
    }

    #[cfg(test)]
    fn epoch_key_for_test(&self) -> VaultEpochKey {
        self.epoch_key.clone()
    }
}

fn decrypt_envelope_with_epoch(
    envelope: &ObjectEnvelope,
    epoch_key: &VaultEpochKey,
) -> Result<Vec<u8>, VaultError> {
    if envelope.tombstone {
        return Err(VaultError::GrantExpired);
    }
    let aad = object_aad(envelope);
    let wrapped = envelope
        .wrapped_dek
        .as_ref()
        .ok_or(VaultError::GrantExpired)?;
    let payload = envelope.payload.as_ref().ok_or(VaultError::GrantExpired)?;
    let dek = unwrap_record_dek(epoch_key, wrapped, aad.as_bytes())?;
    Ok(decrypt_bytes(dek.as_bytes(), aad.as_bytes(), payload)?)
}

fn validate_header(header: &VaultHeader) -> Result<(), VaultError> {
    if header.format != VAULT_FORMAT || header.version != VAULT_VERSION {
        return Err(VaultError::UnsupportedVersion);
    }
    Ok(())
}

fn decrypt_root_key(
    wrapping_key: &aipass_crypto::MasterKey,
    ciphertext: &Ciphertext,
    vault_id: Uuid,
    error: VaultError,
) -> Result<VaultRootKey, VaultError> {
    let mut bytes = decrypt_bytes(
        wrapping_key.as_bytes(),
        root_key_aad(vault_id).as_bytes(),
        ciphertext,
    )
    .map_err(|_| error.clone_like())?;
    if bytes.len() != KEY_LEN {
        bytes.zeroize();
        return Err(error);
    }
    let mut key = [0_u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    bytes.zeroize();
    Ok(VaultRootKey::from_bytes(key))
}

fn root_key_aad(vault_id: Uuid) -> String {
    format!("aipass root key:vault={vault_id}")
}

fn header_key_aad(vault_id: Uuid, key_name: &str) -> String {
    format!("aipass {key_name} key:vault={vault_id}")
}

fn summary_from_plaintext(plaintext: &ProviderRecordPlaintext) -> EntrySummary {
    let entry = &plaintext.entry;
    let primary = entry.secret_refs.first();
    EntrySummary {
        id: entry.id,
        title: entry.title.clone(),
        provider_id: entry.provider_id.clone(),
        provider_kind: entry.provider_kind.clone(),
        domains: entry.domains.clone(),
        favicon_url: entry.favicon_url.clone(),
        endpoints: entry.endpoints.clone(),
        interface_type: entry.interface_type.clone(),
        auth_scheme: entry.auth_scheme.clone(),
        masked_secret: primary
            .map(|secret| secret.masked.clone())
            .unwrap_or_else(|| "****".to_string()),
        fingerprint: primary
            .map(|secret| secret.fingerprint.clone())
            .unwrap_or_default(),
        secret_refs: entry.secret_refs.clone(),
        default_model: entry.default_model.clone(),
        model_aliases: entry.model_aliases.clone(),
        quota: entry.quota.clone(),
        gateway: entry.gateway.clone(),
        tags: entry.tags.clone(),
        notes: entry.notes.clone(),
        header_names: entry.headers.iter().map(|(name, _)| name.clone()).collect(),
        created_at: entry.created_at,
        updated_at: entry.updated_at,
        last_used_at: entry.last_used_at,
        archived_at: entry.archived_at,
        deleted_at: entry.deleted_at,
    }
}

fn plaintext_matches_query(
    plaintext: &ProviderRecordPlaintext,
    query: &str,
    candidate_fingerprint: Option<&str>,
) -> bool {
    if query.is_empty() {
        return true;
    }
    let entry = &plaintext.entry;
    let enum_text = format!("{:?} {:?}", entry.interface_type, entry.auth_scheme).to_lowercase();
    let endpoint_match = entry.endpoints.iter().any(|endpoint| {
        endpoint.id.to_lowercase().contains(query)
            || endpoint
                .url
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
            || endpoint
                .region
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
            || endpoint
                .deployment
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
            || endpoint
                .api_version
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
    });
    let metadata_match = entry.title.to_lowercase().contains(query)
        || entry
            .provider_id
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(query)
        || format!("{:?}", entry.provider_kind)
            .to_lowercase()
            .contains(query)
        || entry
            .domains
            .iter()
            .any(|domain| domain.to_lowercase().contains(query))
        || endpoint_match
        || enum_text.contains(query)
        || entry
            .default_model
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(query)
        || entry.model_aliases.iter().any(|(alias, model)| {
            alias.to_lowercase().contains(query) || model.to_lowercase().contains(query)
        })
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
        || entry
            .notes
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(query)
        || entry
            .headers
            .iter()
            .any(|(name, _)| name.to_lowercase().contains(query))
        || entry.quota.as_ref().is_some_and(|quota| {
            quota
                .label
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
                || quota
                    .limit
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(query)
                || quota
                    .remaining
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(query)
                || quota
                    .reset_at
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(query)
        });
    let gateway_match = entry.gateway.as_ref().is_some_and(|gateway| {
        gateway
            .group
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(query)
            || gateway
                .rate
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(query)
    });
    metadata_match
        || gateway_match
        || entry.secret_refs.iter().any(|secret| {
            secret.masked.to_lowercase().contains(query)
                || secret.fingerprint.to_lowercase().contains(query)
                || candidate_fingerprint == Some(secret.fingerprint.as_str())
        })
}

fn object_aad(envelope: &ObjectEnvelope) -> String {
    format!(
        "vault={};object={};type={};schema={};crypto={};device={};lamport={};updated_at={}",
        envelope.vault_id,
        envelope.object_id,
        envelope.object_type,
        envelope.schema_version,
        envelope.crypto_version,
        envelope.device_id,
        envelope.lamport,
        envelope.updated_at.unix_timestamp_nanos()
    )
}

fn create_dirs(root: &Path) -> Result<(), VaultError> {
    fs::create_dir_all(root.join("objects"))?;
    fs::create_dir_all(root.join("audit"))?;
    fs::create_dir_all(root.join("devices"))?;
    fs::create_dir_all(root.join("grants"))?;
    fs::create_dir_all(root.join("index"))?;
    Ok(())
}

fn exportable_files(root: &Path) -> Result<Vec<PathBuf>, VaultError> {
    let mut files = Vec::new();
    for relative in ["manifest.aipmanifest", "sync-checkpoint.aipcheckpoint"] {
        let path = root.join(relative);
        if path.exists() {
            files.push(PathBuf::from(relative));
        }
    }
    for (dir, ext) in [
        ("objects", "aipobj"),
        ("grants", "aipgrant"),
        ("devices", "aipdevice"),
        ("audit", "aipaudit"),
    ] {
        for path in encrypted_paths(&root.join(dir), ext)? {
            files.push(
                path.strip_prefix(root)
                    .map_err(|_| VaultError::NotFound)?
                    .to_path_buf(),
            );
        }
    }
    files.sort();
    Ok(files)
}

fn checked_relative_path(path: &Path) -> Result<PathBuf, VaultError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(VaultError::NotFound);
    }
    Ok(path.to_path_buf())
}

fn export_aad(vault_id: Uuid, created_at: OffsetDateTime) -> String {
    format!(
        "aipass-export:vault={vault_id};created_at={}",
        created_at.unix_timestamp_nanos()
    )
}

fn encrypted_paths(root: &Path, ext: &str) -> Result<Vec<PathBuf>, VaultError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some(ext) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<(), VaultError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write_bytes(path, &bytes)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T, VaultError> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn scan_for_plaintext(
    root: impl AsRef<Path>,
    needles: &[&str],
) -> Result<Vec<PathBuf>, VaultError> {
    let mut matches = Vec::new();
    scan_dir(root.as_ref(), needles, &mut matches)?;
    Ok(matches)
}

fn scan_dir(path: &Path, needles: &[&str], matches: &mut Vec<PathBuf>) -> Result<(), VaultError> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            scan_dir(&entry?.path(), needles, matches)?;
        }
    } else {
        let bytes = fs::read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        if needles
            .iter()
            .filter(|needle| !needle.is_empty())
            .any(|needle| text.contains(needle))
        {
            matches.push(path.to_path_buf());
        }
    }
    Ok(())
}

pub fn zeroize_string(value: &mut String) {
    value.zeroize();
}

fn redact_detail(value: &str) -> String {
    if value.len() <= 64 && !looks_like_secret(value) {
        value.to_string()
    } else {
        "[redacted]".to_string()
    }
}

fn looks_like_secret(value: &str) -> bool {
    value.contains("sk-")
        || value.contains("sk-ant-")
        || value.contains("AIza")
        || value.to_lowercase().contains("api_key")
        || value.to_lowercase().contains("authorization")
}

fn host_from_origin(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(&trimmed);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split('@')
        .next_back()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_provider_registry::EndpointKind;
    use tempfile::tempdir;

    fn input(secret: &str) -> ProviderEntryInput {
        ProviderEntryInput {
            title: "Anthropic Prod".to_string(),
            provider_kind: ProviderKind::Official,
            provider_id: Some("anthropic".to_string()),
            domains: vec!["console.anthropic.com".to_string()],
            favicon_url: None,
            endpoints: vec![ProviderEndpoint {
                id: "api".to_string(),
                kind: EndpointKind::Api,
                url: Some("https://api.anthropic.com".to_string()),
                region: None,
                deployment: None,
                api_version: None,
            }],
            interface_type: InterfaceType::AnthropicMessages,
            auth_scheme: AuthScheme::XApiKey,
            api_key: secret.to_string(),
            secret_label: None,
            default_model: Some("claude-sonnet-4-5".to_string()),
            model_aliases: Vec::new(),
            headers: vec![("anthropic-version".to_string(), "2023-06-01".to_string())],
            quota: None,
            gateway: None,
            tags: vec!["prod".to_string()],
            notes: Some("sensitive note".to_string()),
        }
    }

    fn update_input(secret: Option<&str>) -> ProviderEntryUpdateInput {
        ProviderEntryUpdateInput {
            title: "Anthropic Prod Renamed".to_string(),
            provider_kind: ProviderKind::Official,
            provider_id: Some("anthropic".to_string()),
            domains: vec!["console.anthropic.com".to_string()],
            favicon_url: Some("https://console.anthropic.com/favicon.ico".to_string()),
            endpoints: vec![ProviderEndpoint {
                id: "api".to_string(),
                kind: EndpointKind::Api,
                url: Some("https://api.anthropic.com".to_string()),
                region: None,
                deployment: None,
                api_version: None,
            }],
            interface_type: InterfaceType::AnthropicMessages,
            auth_scheme: AuthScheme::XApiKey,
            api_key: secret.map(ToString::to_string),
            default_model: Some("claude-opus-4-5".to_string()),
            model_aliases: Vec::new(),
            headers: None,
            quota: Some(QuotaInfo {
                label: Some("team-monthly".to_string()),
                limit: Some("1000000".to_string()),
                remaining: Some("500000".to_string()),
                reset_at: Some("2026-06-01T00:00:00Z".to_string()),
            }),
            gateway: None,
            tags: vec!["prod".to_string(), "team".to_string()],
            notes: Some("renamed without rotating key".to_string()),
        }
    }

    fn test_kdf() -> KdfParams {
        KdfParams::with_random_salt(1024, 1, 1)
    }

    fn create_test_vault(root: &Path, password: &SecretString) -> Vault {
        Vault::create_with_device_and_kdf(root, password, "local device", test_kdf())
            .unwrap()
            .vault
    }

    fn create_test_vault_with_device(
        root: &Path,
        password: &SecretString,
        device_name: &str,
    ) -> Vault {
        Vault::create_with_device_and_kdf(root, password, device_name, test_kdf())
            .unwrap()
            .vault
    }

    #[test]
    fn vault_round_trip_for_non_openai_provider() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        let id = vault
            .add_provider(input("sk-ant-api03-fake-secret-1234"))
            .unwrap();
        let reopened = Vault::open(dir.path(), &password).unwrap();
        assert_eq!(
            reopened.reveal_secret(id).unwrap(),
            "sk-ant-api03-fake-secret-1234"
        );
        let summaries = reopened.search("anthropic").unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].auth_scheme, AuthScheme::XApiKey);
    }

    #[test]
    fn search_by_full_api_key_matches_fingerprint_without_leak() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        vault
            .add_provider(input("sk-ant-api03-fingerprint-secret-1234"))
            .unwrap();
        let matches = vault
            .search("sk-ant-api03-fingerprint-secret-1234")
            .unwrap();
        assert_eq!(matches.len(), 1);
        let serialized = serde_json::to_string(&matches).unwrap();
        assert!(!serialized.contains("sk-ant-api03-fingerprint-secret-1234"));
        assert!(serialized.contains("sk-ant...1234"));
    }

    #[test]
    fn search_matches_model_aliases_and_quota_fields() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        let mut input = input("sk-ant-api03-searchable");
        input.model_aliases = vec![("fast".to_string(), "claude-haiku-4-5".to_string())];
        input.quota = Some(QuotaInfo {
            label: Some("team-monthly".to_string()),
            limit: Some("1000000".to_string()),
            remaining: Some("120000".to_string()),
            reset_at: Some("2026-06-30".to_string()),
        });
        input.gateway = Some(GatewayMetadata {
            group: Some("vip".to_string()),
            rate: Some("0.8x".to_string()),
        });
        let id = vault.add_provider(input).unwrap();

        for query in [
            "fast",
            "claude-haiku-4-5",
            "120000",
            "2026-06-30",
            "vip",
            "0.8x",
        ] {
            let matches = vault.search(query).unwrap();
            assert_eq!(matches.len(), 1, "query {query}");
            assert_eq!(matches[0].id, id, "query {query}");
        }
    }

    #[test]
    fn update_without_api_key_preserves_existing_secret_and_headers() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        let id = vault.add_provider(input("sk-ant-api03-original")).unwrap();
        vault.update_provider(id, update_input(None)).unwrap();
        let summary = vault.get_provider_summary(id).unwrap();
        assert_eq!(summary.title, "Anthropic Prod Renamed");
        assert_eq!(summary.default_model.as_deref(), Some("claude-opus-4-5"));
        assert!(summary
            .header_names
            .iter()
            .any(|name| name == "anthropic-version"));
        assert_eq!(vault.reveal_secret(id).unwrap(), "sk-ant-api03-original");
    }

    #[test]
    fn favicon_backfill_only_sets_missing_url_and_preserves_record_fields() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        let id = vault.add_provider(input("sk-ant-api03-favicon")).unwrap();

        let updated = vault
            .set_provider_favicon_url(id, "https://console.anthropic.com/favicon.ico")
            .unwrap()
            .expect("missing favicon should update");
        assert_eq!(
            updated.favicon_url.as_deref(),
            Some("https://console.anthropic.com/favicon.ico")
        );
        assert!(updated
            .header_names
            .iter()
            .any(|name| name == "anthropic-version"));
        assert_eq!(vault.reveal_secret(id).unwrap(), "sk-ant-api03-favicon");

        assert!(vault
            .set_provider_favicon_url(id, "https://example.com/favicon.ico")
            .unwrap()
            .is_none());
        let reopened = Vault::open(dir.path(), &password).unwrap();
        let summary = reopened.get_provider_summary(id).unwrap();
        assert_eq!(
            summary.favicon_url.as_deref(),
            Some("https://console.anthropic.com/favicon.ico")
        );
        assert_eq!(reopened.reveal_secret(id).unwrap(), "sk-ant-api03-favicon");
    }

    #[test]
    fn multi_secret_records_can_add_reveal_and_remove_secondary_keys() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        let id = vault.add_provider(input("sk-ant-api03-primary")).unwrap();

        let fallback_id = vault
            .add_secret(id, "fallback", "sk-ant-api03-fallback")
            .unwrap();
        let summary = vault.get_provider_summary(id).unwrap();
        assert_eq!(summary.secret_refs.len(), 2);
        assert!(summary.secret_refs.iter().any(|secret| {
            secret.id == fallback_id
                && secret.label == "fallback"
                && secret.masked.ends_with("back")
        }));
        assert_eq!(
            vault.reveal_secret_field(id, "fallback").unwrap(),
            "sk-ant-api03-fallback"
        );
        assert_eq!(
            vault.reveal_secret_field(id, &fallback_id).unwrap(),
            "sk-ant-api03-fallback"
        );
        assert!(matches!(
            vault.add_secret(id, "fallback", "sk-ant-api03-duplicate"),
            Err(VaultError::DuplicateSecretLabel)
        ));

        vault.remove_secret(id, "fallback").unwrap();
        let summary = vault.get_provider_summary(id).unwrap();
        assert_eq!(summary.secret_refs.len(), 1);
        assert!(matches!(
            vault.remove_secret(id, "primary"),
            Err(VaultError::LastSecret)
        ));
    }

    #[test]
    fn stolen_vault_scan_does_not_find_provider_plaintext() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        vault
            .add_provider(input("sk-ant-api03-fake-secret-1234"))
            .unwrap();
        let matches = scan_for_plaintext(
            dir.path(),
            &[
                "sk-ant-api03-fake-secret-1234",
                "Anthropic Prod",
                "api.anthropic.com",
                "sensitive note",
                "anthropic_messages",
                "x_api_key",
            ],
        )
        .unwrap();
        assert!(matches.is_empty(), "plaintext found in {matches:?}");
    }

    #[test]
    fn encrypted_export_round_trip_does_not_leak_plaintext_metadata() {
        let dir = tempdir().unwrap();
        let import_dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let export_password = SecretString::new("export-only passphrase");
        let vault = create_test_vault(dir.path(), &password);
        let id = vault
            .add_provider(input("sk-ant-api03-export-secret-1234"))
            .unwrap();
        vault
            .add_secret(id, "fallback", "sk-ant-api03-export-fallback")
            .unwrap();

        let export = vault
            .export_encrypted_with_kdf(&export_password, test_kdf())
            .unwrap();
        let export_text = serde_json::to_string_pretty(&export).unwrap();
        for needle in [
            "sk-ant-api03-export-secret-1234",
            "sk-ant-api03-export-fallback",
            "Anthropic Prod",
            "api.anthropic.com",
            "sensitive note",
            "anthropic_messages",
            "x_api_key",
        ] {
            assert!(
                !export_text.contains(needle),
                "export leaked plaintext needle {needle}"
            );
        }

        Vault::import_encrypted(import_dir.path(), &export_password, &export).unwrap();
        let imported = Vault::open(import_dir.path(), &password).unwrap();
        assert_eq!(
            imported.reveal_secret(id).unwrap(),
            "sk-ant-api03-export-secret-1234"
        );
        assert_eq!(
            imported.reveal_secret_field(id, "fallback").unwrap(),
            "sk-ant-api03-export-fallback"
        );
        assert!(Vault::import_encrypted(
            tempdir().unwrap().path(),
            &SecretString::new("wrong"),
            &export
        )
        .is_err());
    }

    #[test]
    fn wrong_password_fails() {
        let dir = tempdir().unwrap();
        create_test_vault(dir.path(), &SecretString::new("right"));
        assert!(Vault::open(dir.path(), &SecretString::new("wrong")).is_err());
    }

    #[test]
    fn password_rotation_invalidates_old_password_and_keeps_records() {
        let dir = tempdir().unwrap();
        let mut vault = create_test_vault(dir.path(), &SecretString::new("old password"));
        let id = vault
            .add_provider(input("sk-ant-api03-fake-secret-1234"))
            .unwrap();
        vault
            .change_master_password_with_kdf(&SecretString::new("new password"), test_kdf())
            .unwrap();
        assert!(Vault::open(dir.path(), &SecretString::new("old password")).is_err());
        let reopened = Vault::open(dir.path(), &SecretString::new("new password")).unwrap();
        assert_eq!(
            reopened.reveal_secret(id).unwrap(),
            "sk-ant-api03-fake-secret-1234"
        );
    }

    #[test]
    fn recovery_key_resets_password_and_is_not_persisted() {
        let dir = tempdir().unwrap();
        let creation = Vault::create_with_device_and_kdf(
            dir.path(),
            &SecretString::new("old password"),
            "local device",
            test_kdf(),
        )
        .unwrap();
        let recovery_key = creation.recovery_kit.recovery_key.clone();
        let vault = creation.vault;
        let before_epoch = vault.current_epoch().epoch;
        let id = vault
            .add_provider(input("sk-ant-api03-recovery-secret"))
            .unwrap();
        drop(vault);

        let matches = scan_for_plaintext(dir.path(), &[&recovery_key]).unwrap();
        assert!(matches.is_empty(), "recovery key found in {matches:?}");
        assert!(Vault::recover_master_password_with_kdf(
            dir.path(),
            &SecretString::new("bad recovery key"),
            &SecretString::new("new password"),
            test_kdf(),
        )
        .is_err());

        let recovered = Vault::recover_master_password_with_kdf(
            dir.path(),
            &SecretString::new(&recovery_key),
            &SecretString::new("new password"),
            test_kdf(),
        )
        .unwrap();
        let new_recovery_key = recovered.recovery_kit.recovery_key.clone();
        assert_ne!(recovery_key, new_recovery_key);
        assert!(recovered.vault.current_epoch().epoch > before_epoch);
        assert_eq!(
            recovered.vault.reveal_secret(id).unwrap(),
            "sk-ant-api03-recovery-secret"
        );
        drop(recovered);

        assert!(Vault::open(dir.path(), &SecretString::new("old password")).is_err());
        assert!(Vault::open(dir.path(), &SecretString::new("new password")).is_ok());
        assert!(Vault::recover_master_password_with_kdf(
            dir.path(),
            &SecretString::new(&recovery_key),
            &SecretString::new("another password"),
            test_kdf(),
        )
        .is_err());
    }

    #[test]
    fn tamper_test_fails_decrypt() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        let id = vault
            .add_provider(input("sk-ant-api03-fake-secret-1234"))
            .unwrap();
        let path = dir.path().join("objects").join(format!("{id}.aipobj"));
        let mut text = fs::read_to_string(&path).unwrap();
        text = text.replacen("provider_entry", "other_recordzz", 1);
        fs::write(&path, text).unwrap();
        let reopened = Vault::open(dir.path(), &password).unwrap();
        assert!(reopened.reveal_secret(id).is_err());
    }

    #[test]
    fn ttl_erasure_test_keeps_active_record_but_erases_grant() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let vault = create_test_vault(dir.path(), &password);
        let id = vault
            .add_provider(input("sk-ant-api03-fake-secret-1234"))
            .unwrap();
        let grant = vault
            .create_secret_grant(
                id,
                "chrome.fill",
                60,
                Some("https://console.anthropic.com".to_string()),
            )
            .unwrap();
        assert_eq!(
            vault.consume_secret_grant(grant.id).unwrap(),
            "sk-ant-api03-fake-secret-1234"
        );
        vault.expire_grant(grant.id).unwrap();
        assert!(matches!(
            vault.consume_secret_grant(grant.id),
            Err(VaultError::GrantExpired)
        ));
        assert_eq!(
            vault.reveal_secret(id).unwrap(),
            "sk-ant-api03-fake-secret-1234"
        );
    }

    #[test]
    fn compromise_recovery_test_old_epoch_cannot_decrypt_new_writes() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let mut vault = create_test_vault(dir.path(), &password);
        vault
            .add_provider(input("sk-ant-api03-old-secret"))
            .unwrap();
        let leaked_epoch = vault.epoch_key_for_test();
        vault.advance_epoch_and_rewrap("manual.rotate").unwrap();
        let new_id = vault
            .add_provider(input("sk-ant-api03-new-secret"))
            .unwrap();
        assert!(vault
            .decrypt_envelope_with_epoch_for_test(new_id, &leaked_epoch)
            .is_err());
        assert_eq!(
            vault.reveal_secret(new_id).unwrap(),
            "sk-ant-api03-new-secret"
        );
    }

    #[test]
    fn device_revoke_advances_epoch() {
        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let mut vault = create_test_vault_with_device(dir.path(), &password, "MacBook");
        let device_id = vault.current_device_id();
        let before = vault.current_epoch().epoch;
        vault.revoke_device(device_id).unwrap();
        assert!(vault.current_epoch().epoch > before);
        let devices = vault.list_devices().unwrap();
        assert!(devices
            .iter()
            .any(|device| device.id == device_id && !device.trusted));
    }
}
