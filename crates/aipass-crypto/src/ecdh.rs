//! Ephemeral ECDH key exchange for secure remote unlock protocol.
//!
//! Uses X25519 curve for forward secrecy in challenge-response authentication.

use crate::CryptoError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand_core::{OsRng, RngCore};
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

/// Ephemeral private key for ECDH exchange.
///
/// Automatically zeroized on drop for forward secrecy.
pub struct EphemeralPrivateKey(EphemeralSecret);

/// Ephemeral public key for ECDH exchange.
#[derive(Clone)]
pub struct EphemeralPublicKey([u8; 32]);

impl EphemeralPrivateKey {
    /// Generate a new random ephemeral keypair.
    pub fn generate() -> (Self, EphemeralPublicKey) {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        (
            EphemeralPrivateKey(secret),
            EphemeralPublicKey(public.to_bytes()),
        )
    }

    /// Compute shared secret with peer's public key.
    ///
    /// Consumes the private key for forward secrecy.
    pub fn shared_secret(
        self,
        peer_public: &EphemeralPublicKey,
    ) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
        let peer_key = PublicKey::from(peer_public.0);
        let shared = self.0.diffie_hellman(&peer_key);
        if !shared.was_contributory() {
            return Err(CryptoError::NonContributoryPublicKey);
        }
        Ok(Zeroizing::new(shared.to_bytes()))
    }
}

impl EphemeralPublicKey {
    /// Create from raw 32-byte public key.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Export as raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Export to base64url encoding for transmission.
    pub fn to_base64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Parse from base64url encoding.
    pub fn from_base64(s: &str) -> Result<Self, CryptoError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|_| CryptoError::InvalidEncoding)?;
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidKeyLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

/// Generate a random 32-byte nonce for challenge-response.
pub fn generate_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecdh_round_trip() {
        // Alice generates keypair
        let (alice_priv, alice_pub) = EphemeralPrivateKey::generate();

        // Bob generates keypair
        let (bob_priv, bob_pub) = EphemeralPrivateKey::generate();

        // Both compute shared secret
        let alice_shared = alice_priv.shared_secret(&bob_pub).unwrap();
        let bob_shared = bob_priv.shared_secret(&alice_pub).unwrap();

        // Shared secrets must match
        assert_eq!(alice_shared, bob_shared);
        assert_ne!(*alice_shared, [0u8; 32]);
    }

    #[test]
    fn public_key_base64_round_trip() {
        let (_, public) = EphemeralPrivateKey::generate();
        let encoded = public.to_base64();
        let decoded = EphemeralPublicKey::from_base64(&encoded).unwrap();
        assert_eq!(public.as_bytes(), decoded.as_bytes());
    }

    #[test]
    fn nonce_generation() {
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();

        // Nonces should be random and different
        assert_ne!(nonce1, nonce2);
        assert_ne!(nonce1, [0u8; 32]);
    }

    #[test]
    fn invalid_base64_rejected() {
        assert!(EphemeralPublicKey::from_base64("not-valid!").is_err());
        assert!(EphemeralPublicKey::from_base64("dG9vc2hvcnQ").is_err()); // too short
    }

    #[test]
    fn rejects_low_order_public_keys() {
        for bytes in [[0u8; 32], {
            let mut bytes = [0u8; 32];
            bytes[0] = 1;
            bytes
        }] {
            let (private, _) = EphemeralPrivateKey::generate();
            assert!(matches!(
                private.shared_secret(&EphemeralPublicKey::from_bytes(bytes)),
                Err(CryptoError::NonContributoryPublicKey)
            ));
        }
    }
}
