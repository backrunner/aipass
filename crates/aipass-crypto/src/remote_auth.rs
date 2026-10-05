//! Challenge-response primitives for high-entropy remote access codes.
//!
//! This is shared-secret HMAC authentication, not SRP or a zero-knowledge protocol.
//! The verifier grants authentication authority and must be encrypted at rest.
//! Callers must authenticate the server, expire challenges, and consume each
//! challenge once; these primitives alone do not implement a remote unlock flow.

use crate::{CryptoError, EphemeralPublicKey};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use zeroize::{ZeroizeOnDrop, Zeroizing};

type HmacSha256 = Hmac<Sha256>;

/// 512-bit secure access code (64 bytes).
///
/// Automatically zeroized on drop to prevent memory disclosure.
#[derive(Clone, ZeroizeOnDrop)]
pub struct SecureAccessCode([u8; 64]);

impl SecureAccessCode {
    /// Generate a new random 512-bit access code.
    pub fn generate() -> Self {
        let mut code = Self([0u8; 64]);
        OsRng.fill_bytes(&mut code.0);
        code
    }

    /// Create from raw 64-byte array.
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Export as base64url (86 characters, no padding).
    pub fn to_base64(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(self.0))
    }

    /// Parse from base64url encoding.
    pub fn from_base64(s: &str) -> Result<Self, CryptoError> {
        let bytes = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(s)
                .map_err(|_| CryptoError::InvalidEncoding)?,
        );
        if bytes.len() != 64 {
            return Err(CryptoError::InvalidKeyLength);
        }
        let mut code = Self([0u8; 64]);
        code.0.copy_from_slice(&bytes);
        Ok(code)
    }

    /// Access raw bytes (use with care).
    pub(crate) fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

/// Derive authentication secret from access code.
///
/// Domain: "aipass.secure-unlock.auth.v1"
pub fn derive_auth_secret(
    code: &SecureAccessCode,
    salt: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), code.as_bytes());
    let mut secret = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"aipass.secure-unlock.auth.v1", &mut *secret)
        .map_err(|_| CryptoError::KeyDerivationFailed)?;
    Ok(secret)
}

/// Derive wrapping key for vault root key encryption.
///
/// Domain: "aipass.secure-unlock.wrap.v1"
pub fn derive_wrap_key(
    code: &SecureAccessCode,
    salt: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), code.as_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"aipass.secure-unlock.wrap.v1", &mut *key)
        .map_err(|_| CryptoError::KeyDerivationFailed)?;
    Ok(key)
}

/// Derive session key from ECDH shared secret.
///
/// Domain: "aipass.unlock-session.v1"
pub fn derive_session_key(
    shared_secret: &[u8; 32],
    nonce: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    if shared_secret == &[0u8; 32] {
        return Err(CryptoError::NonContributoryPublicKey);
    }
    let hkdf = Hkdf::<Sha256>::new(Some(nonce), shared_secret);
    let mut key = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"aipass.unlock-session.v1", &mut *key)
        .map_err(|_| CryptoError::KeyDerivationFailed)?;
    Ok(key)
}

/// Derive the server's authentication key from the access-code auth secret.
///
/// This verifier is a secret credential, not a public password hash. Possession
/// permits generating proofs, so callers must encrypt it at rest and zeroize it.
pub fn compute_verifier(auth_secret: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    let mut mac = HmacSha256::new_from_slice(auth_secret).expect("HMAC accepts any key length");
    mac.update(b"aipass.secure-unlock.verifier.v1");
    Zeroizing::new(mac.finalize().into_bytes().into())
}

/// Generate a proof binding the challenge and both ephemeral public keys.
pub fn generate_proof(
    auth_secret: &[u8; 32],
    challenge_nonce: &[u8; 32],
    server_pubkey: &EphemeralPublicKey,
    client_pubkey: &EphemeralPublicKey,
) -> [u8; 32] {
    let verifier = compute_verifier(auth_secret);
    proof_mac(&verifier, challenge_nonce, server_pubkey, client_pubkey)
        .finalize()
        .into_bytes()
        .into()
}

/// Verify a proof using the secret returned by `compute_verifier`.
pub fn verify_proof(
    proof: &[u8; 32],
    stored_verifier: &[u8; 32],
    challenge_nonce: &[u8; 32],
    server_pubkey: &EphemeralPublicKey,
    client_pubkey: &EphemeralPublicKey,
) -> bool {
    proof_mac(
        stored_verifier,
        challenge_nonce,
        server_pubkey,
        client_pubkey,
    )
    .verify_slice(proof)
    .is_ok()
}

fn proof_mac(
    verifier: &[u8; 32],
    challenge_nonce: &[u8; 32],
    server_pubkey: &EphemeralPublicKey,
    client_pubkey: &EphemeralPublicKey,
) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(verifier).expect("HMAC accepts any key length");
    mac.update(b"aipass.secure-unlock.proof.v1");
    mac.update(challenge_nonce);
    mac.update(server_pubkey.as_bytes());
    mac.update(client_pubkey.as_bytes());
    mac
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecdh::EphemeralPrivateKey;

    #[test]
    fn access_code_generation() {
        let code1 = SecureAccessCode::generate();
        let code2 = SecureAccessCode::generate();

        // Codes should be random and different
        assert_ne!(code1.as_bytes(), code2.as_bytes());
        assert_ne!(*code1.as_bytes(), [0u8; 64]);

        // Base64 encoding should be 86 characters (no padding)
        let encoded = code1.to_base64();
        assert_eq!(encoded.len(), 86);
        assert!(!encoded.contains('='));
    }

    #[test]
    fn access_code_base64_round_trip() {
        let code = SecureAccessCode::generate();
        let encoded = code.to_base64();
        let decoded = SecureAccessCode::from_base64(&encoded).unwrap();
        assert_eq!(code.as_bytes(), decoded.as_bytes());
    }

    #[test]
    fn key_derivation_domains() {
        let code = SecureAccessCode::generate();
        let salt = [42u8; 32];

        let auth_secret = derive_auth_secret(&code, &salt).unwrap();
        let wrap_key = derive_wrap_key(&code, &salt).unwrap();

        // Different domain strings produce different keys
        assert_ne!(auth_secret, wrap_key);
        assert_ne!(*auth_secret, [0u8; 32]);
        assert_ne!(*wrap_key, [0u8; 32]);
    }

    #[test]
    fn session_key_derivation() {
        let shared_secret = [1u8; 32];
        let nonce = [2u8; 32];

        let session_key = derive_session_key(&shared_secret, &nonce).unwrap();
        assert_ne!(*session_key, shared_secret);
        assert_ne!(*session_key, nonce);
        assert_ne!(*session_key, [0u8; 32]);
    }

    #[test]
    fn proof_verification_round_trip() {
        let code = SecureAccessCode::generate();
        let salt = [3u8; 32];
        let nonce = [4u8; 32];

        let auth_secret = derive_auth_secret(&code, &salt).unwrap();

        let (_, server_pub) = EphemeralPrivateKey::generate();
        let (_, client_pub) = EphemeralPrivateKey::generate();

        // Client generates proof
        let proof = generate_proof(&auth_secret, &nonce, &server_pub, &client_pub);

        // Server verifies using exactly the verifier returned by the public API.
        let verifier = compute_verifier(&auth_secret);
        let valid = verify_proof(&proof, &verifier, &nonce, &server_pub, &client_pub);
        assert!(valid);

        // Wrong proof should fail
        let wrong_proof = [99u8; 32];
        let invalid = verify_proof(&wrong_proof, &verifier, &nonce, &server_pub, &client_pub);
        assert!(!invalid);

        // Wrong nonce should fail
        let wrong_nonce = [5u8; 32];
        let invalid = verify_proof(&proof, &verifier, &wrong_nonce, &server_pub, &client_pub);
        assert!(!invalid);
        let (_, other_pub) = EphemeralPrivateKey::generate();
        assert!(!verify_proof(
            &proof,
            &verifier,
            &nonce,
            &other_pub,
            &client_pub
        ));
        assert!(!verify_proof(
            &proof,
            &verifier,
            &nonce,
            &server_pub,
            &other_pub
        ));
        assert!(!verify_proof(
            &proof,
            &verifier,
            &nonce,
            &client_pub,
            &server_pub
        ));
        assert!(!verify_proof(
            &proof,
            &compute_verifier(&[9u8; 32]),
            &nonce,
            &server_pub,
            &client_pub
        ));
        assert!(!verify_proof(
            &proof,
            &auth_secret,
            &nonce,
            &server_pub,
            &client_pub
        ));
    }

    #[test]
    fn rejects_zero_shared_secret_and_invalid_codes() {
        assert!(matches!(
            derive_session_key(&[0u8; 32], &[2u8; 32]),
            Err(CryptoError::NonContributoryPublicKey)
        ));
        assert!(SecureAccessCode::from_base64("not-valid!").is_err());
        assert!(SecureAccessCode::from_base64(&URL_SAFE_NO_PAD.encode([0u8; 63])).is_err());
        assert!(SecureAccessCode::from_base64(&URL_SAFE_NO_PAD.encode([0u8; 65])).is_err());
    }

    #[test]
    fn verifier_computation() {
        let auth_secret = [6u8; 32];
        let verifier1 = compute_verifier(&auth_secret);
        let verifier2 = compute_verifier(&auth_secret);

        // Deterministic
        assert_eq!(verifier1, verifier2);
        assert_ne!(*verifier1, auth_secret);
        assert_ne!(*verifier1, [0u8; 32]);
    }
}
