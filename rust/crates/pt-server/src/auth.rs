//! Passwords (Argon2id, PHC strings) and session tokens.
//!
//! Hashes must verify across the Rust and Go servers, so the parameters and
//! encoding are fixed by protocol §1: m=19456 KiB, t=2, p=1, 16-byte salt,
//! 32-byte output, standard base64 without padding.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

pub const SESSION_TTL_SECS: i64 = 14 * 24 * 3600;

fn argon() -> Argon2<'static> {
    // Constant, valid parameters: unwrap cannot fail.
    let params = Params::new(19456, 2, 1, Some(32)).expect("argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    argon()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing failed: {e}"))
}

/// Constant-time verification (inside argon2). Any parse error is a mismatch.
pub fn verify_password(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => argon().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// A hash to verify against when the user does not exist, so a login for an
/// unknown user costs the same time as a wrong password.
pub fn dummy_hash() -> &'static str {
    use std::sync::OnceLock;
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| hash_password("not-a-real-password-just-timing").unwrap_or_default())
}

/// 32 random bytes, base64url without padding (43 chars).
pub fn new_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

pub fn token_hash(token: &str) -> String {
    let d = Sha256::digest(token.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// A random human-typeable password for `pt seed` (no ambiguous characters).
pub fn random_password(len: usize) -> String {
    const ALPHA: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut out = String::with_capacity(len);
    let mut b = [0u8; 1];
    while out.len() < len {
        OsRng.fill_bytes(&mut b);
        // rejection sampling keeps the distribution uniform
        if (b[0] as usize) < 256 - 256 % ALPHA.len() {
            out.push(ALPHA[b[0] as usize % ALPHA.len()] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_roundtrip() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "garbage"));
    }

    #[test]
    fn tokens() {
        let t = new_token();
        assert_eq!(t.len(), 43);
        assert_ne!(t, new_token());
        assert_eq!(token_hash("abc").len(), 64);
        assert_eq!(random_password(12).len(), 12);
    }
}
