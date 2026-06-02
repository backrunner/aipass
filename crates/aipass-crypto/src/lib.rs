use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const SALT_LEN: usize = 16;
pub const RECOVERY_SECRET_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid KDF parameters")]
    InvalidKdfParams,
    #[error("key derivation failed")]
    KdfFailed,
    #[error("encryption failed")]
    EncryptFailed,
    #[error("decryption failed")]
    DecryptFailed,
    #[error("key derivation output failed")]
    HkdfFailed,
    #[error("invalid key material length")]
    InvalidKeyLength,
    #[error("invalid recovery secret")]
    InvalidRecoverySecret,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdfParams {
    pub algorithm: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt_b64: String,
}

impl KdfParams {
    pub fn interactive() -> Self {
        Self::with_random_salt(64 * 1024, 2, 1)
    }

    pub fn with_random_salt(memory_kib: u32, iterations: u32, parallelism: u32) -> Self {
        let mut salt = [0_u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        Self {
            algorithm: "argon2id".to_string(),
            memory_kib,
            iterations,
            parallelism,
            salt_b64: STANDARD_NO_PAD.encode(salt),
        }
    }

    fn salt(&self) -> Result<Vec<u8>, CryptoError> {
        STANDARD_NO_PAD
            .decode(self.salt_b64.as_bytes())
            .map_err(|_| CryptoError::InvalidKdfParams)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ciphertext {
    pub aead: String,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultEpoch {
    pub epoch: u64,
    pub key_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey([u8; KEY_LEN]);

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct VaultRootKey([u8; KEY_LEN]);

#[derive(Clone)]
pub struct VaultEpochKey {
    epoch: VaultEpoch,
    key: [u8; KEY_LEN],
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct RecordDek([u8; KEY_LEN]);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WrappedDek {
    pub epoch: u64,
    pub key_id: Uuid,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

impl MasterKey {
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl VaultRootKey {
    pub fn generate() -> Self {
        let mut key = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl VaultEpochKey {
    pub fn new(epoch: u64) -> Self {
        let mut key = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        Self {
            epoch: VaultEpoch {
                epoch,
                key_id: Uuid::new_v4(),
                created_at: OffsetDateTime::now_utc(),
            },
            key,
        }
    }

    pub fn from_parts(epoch: VaultEpoch, key: [u8; KEY_LEN]) -> Self {
        Self { epoch, key }
    }

    pub fn epoch(&self) -> &VaultEpoch {
        &self.epoch
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.key
    }
}

impl Drop for VaultEpochKey {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl RecordDek {
    pub fn generate() -> Self {
        let mut key = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

pub fn derive_master_key(
    password: &SecretString,
    params: &KdfParams,
) -> Result<MasterKey, CryptoError> {
    if params.algorithm != "argon2id" {
        return Err(CryptoError::InvalidKdfParams);
    }
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|_| CryptoError::InvalidKdfParams)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut out = [0_u8; KEY_LEN];
    argon2
        .hash_password_into(password.expose().as_bytes(), &params.salt()?, &mut out)
        .map_err(|_| CryptoError::KdfFailed)?;
    Ok(MasterKey(out))
}

pub fn generate_recovery_secret() -> String {
    let mut secret = [0_u8; RECOVERY_SECRET_LEN];
    OsRng.fill_bytes(&mut secret);
    let encoded = hex_encode_upper(&secret);
    secret.zeroize();
    let grouped = encoded
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).expect("hex is ascii"))
        .collect::<Vec<_>>()
        .join("-");
    format!("AIPASS-{grouped}")
}

pub fn derive_recovery_key(recovery_secret: &SecretString) -> Result<MasterKey, CryptoError> {
    let normalized = normalize_recovery_secret(recovery_secret.expose());
    let mut secret = hex_decode(&normalized)?;
    if secret.len() != RECOVERY_SECRET_LEN {
        secret.zeroize();
        return Err(CryptoError::InvalidRecoverySecret);
    }
    let hk = Hkdf::<Sha256>::new(None, &secret);
    let mut out = [0_u8; KEY_LEN];
    hk.expand(b"aipass recovery root key wrap v1", &mut out)
        .map_err(|_| CryptoError::HkdfFailed)?;
    secret.zeroize();
    Ok(MasterKey(out))
}

fn normalize_recovery_secret(value: &str) -> String {
    let trimmed = value.trim();
    let without_prefix = trimmed
        .strip_prefix("AIPASS-")
        .or_else(|| trimmed.strip_prefix("aipass-"))
        .unwrap_or(trimmed);
    without_prefix
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace() && *ch != '-')
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
}

fn hex_encode_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>, CryptoError> {
    if value.len() != RECOVERY_SECRET_LEN * 2 {
        return Err(CryptoError::InvalidRecoverySecret);
    }
    let mut out = vec![0_u8; RECOVERY_SECRET_LEN];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = match hex_value(chunk[0]) {
            Some(value) => value,
            None => {
                out.zeroize();
                return Err(CryptoError::InvalidRecoverySecret);
            }
        };
        let low = match hex_value(chunk[1]) {
            Some(value) => value,
            None => {
                out.zeroize();
                return Err(CryptoError::InvalidRecoverySecret);
            }
        };
        out[index] = (high << 4) | low;
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn generate_record_dek() -> RecordDek {
    RecordDek::generate()
}

pub fn advance_epoch(current: &VaultEpochKey) -> Result<VaultEpochKey, CryptoError> {
    let mut fresh = [0_u8; KEY_LEN];
    OsRng.fill_bytes(&mut fresh);
    let hk = Hkdf::<Sha256>::new(Some(&fresh), current.as_bytes());
    let mut next = [0_u8; KEY_LEN];
    hk.expand(b"aipass vault epoch advance", &mut next)
        .map_err(|_| CryptoError::HkdfFailed)?;
    Ok(VaultEpochKey::from_parts(
        VaultEpoch {
            epoch: current.epoch.epoch + 1,
            key_id: Uuid::new_v4(),
            created_at: OffsetDateTime::now_utc(),
        },
        next,
    ))
}

pub fn encrypt_bytes(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Ciphertext, CryptoError> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::EncryptFailed)?;
    Ok(Ciphertext {
        aead: "xchacha20poly1305".to_string(),
        nonce_b64: STANDARD_NO_PAD.encode(nonce),
        ciphertext_b64: STANDARD_NO_PAD.encode(ciphertext),
    })
}

pub fn decrypt_bytes(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    ciphertext: &Ciphertext,
) -> Result<Vec<u8>, CryptoError> {
    if ciphertext.aead != "xchacha20poly1305" {
        return Err(CryptoError::DecryptFailed);
    }
    let nonce = STANDARD_NO_PAD
        .decode(ciphertext.nonce_b64.as_bytes())
        .map_err(|_| CryptoError::DecryptFailed)?;
    let bytes = STANDARD_NO_PAD
        .decode(ciphertext.ciphertext_b64.as_bytes())
        .map_err(|_| CryptoError::DecryptFailed)?;
    if nonce.len() != NONCE_LEN {
        return Err(CryptoError::DecryptFailed);
    }
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload { msg: &bytes, aad },
        )
        .map_err(|_| CryptoError::DecryptFailed)
}

pub fn wrap_record_dek(
    epoch_key: &VaultEpochKey,
    dek: &RecordDek,
    aad: &[u8],
) -> Result<WrappedDek, CryptoError> {
    let encrypted = encrypt_bytes(epoch_key.as_bytes(), aad, dek.as_bytes())?;
    Ok(WrappedDek {
        epoch: epoch_key.epoch.epoch,
        key_id: epoch_key.epoch.key_id,
        nonce_b64: encrypted.nonce_b64,
        ciphertext_b64: encrypted.ciphertext_b64,
    })
}

pub fn unwrap_record_dek(
    epoch_key: &VaultEpochKey,
    wrapped: &WrappedDek,
    aad: &[u8],
) -> Result<RecordDek, CryptoError> {
    if wrapped.epoch != epoch_key.epoch.epoch || wrapped.key_id != epoch_key.epoch.key_id {
        return Err(CryptoError::DecryptFailed);
    }
    let bytes = decrypt_bytes(
        epoch_key.as_bytes(),
        aad,
        &Ciphertext {
            aead: "xchacha20poly1305".to_string(),
            nonce_b64: wrapped.nonce_b64.clone(),
            ciphertext_b64: wrapped.ciphertext_b64.clone(),
        },
    )?;
    if bytes.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength);
    }
    let mut key = [0_u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(RecordDek(key))
}

pub fn hmac_fingerprint(index_key: &[u8; KEY_LEN], secret: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(index_key)
        .expect("HMAC accepts fixed-size key material");
    mac.update(secret.as_bytes());
    let out = mac.finalize().into_bytes();
    STANDARD_NO_PAD.encode(&out[..12])
}

pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.is_empty() {
        return "****".to_string();
    }
    if chars.len() <= 8 {
        let head: String = chars.iter().take(2).copied().collect();
        let tail: String = chars
            .iter()
            .rev()
            .take(2)
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return format!("{head}...{tail}");
    }
    let head: String = chars.iter().take(6).copied().collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(4)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}...{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_round_trip_and_tamper_fails() {
        let epoch = VaultEpochKey::new(0);
        let aad = b"record-id";
        let encrypted = encrypt_bytes(epoch.as_bytes(), aad, b"secret").unwrap();
        assert_eq!(
            decrypt_bytes(epoch.as_bytes(), aad, &encrypted).unwrap(),
            b"secret"
        );

        let mut tampered = encrypted.clone();
        tampered.ciphertext_b64.push('A');
        assert!(decrypt_bytes(epoch.as_bytes(), aad, &tampered).is_err());
        assert!(decrypt_bytes(epoch.as_bytes(), b"other", &encrypted).is_err());
    }

    #[test]
    fn epoch_ratchet_blocks_old_key_from_new_data() {
        let old = VaultEpochKey::new(0);
        let new = advance_epoch(&old).unwrap();
        let encrypted = encrypt_bytes(new.as_bytes(), b"aad", b"future").unwrap();
        assert!(decrypt_bytes(old.as_bytes(), b"aad", &encrypted).is_err());
        assert_eq!(
            decrypt_bytes(new.as_bytes(), b"aad", &encrypted).unwrap(),
            b"future"
        );
    }

    #[test]
    fn wrap_and_unwrap_record_dek() {
        let epoch = VaultEpochKey::new(1);
        let dek = generate_record_dek();
        let wrapped = wrap_record_dek(&epoch, &dek, b"record").unwrap();
        let unwrapped = unwrap_record_dek(&epoch, &wrapped, b"record").unwrap();
        assert_eq!(dek.as_bytes(), unwrapped.as_bytes());
        assert!(unwrap_record_dek(&epoch, &wrapped, b"wrong").is_err());
    }

    #[test]
    fn recovery_secret_derives_stable_key() {
        let secret = generate_recovery_secret();
        assert!(secret.starts_with("AIPASS-"));
        let compact = secret.replace('-', "");
        let key = derive_recovery_key(&SecretString::new(&secret)).unwrap();
        let same =
            derive_recovery_key(&SecretString::new(compact.replacen("AIPASS", "AIPASS-", 1)))
                .unwrap();
        assert_eq!(key.as_bytes(), same.as_bytes());
        assert!(derive_recovery_key(&SecretString::new("not-a-valid-key")).is_err());
    }
}
