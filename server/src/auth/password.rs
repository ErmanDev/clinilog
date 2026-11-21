use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use std::sync::OnceLock;

/// Hashes a password with Argon2id and a random salt (PHC string format).
pub fn hash(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("failed to hash password: {e}"))
}

/// Checks a password against a stored hash. Case-sensitive: the exact bytes are compared.
/// A malformed stored hash counts as "no match" rather than an error.
pub fn verify(password: &str, stored_hash: &str) -> bool {
    match PasswordHash::new(stored_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// A real hash of a throwaway value. When a username doesn't exist we still verify
/// against this, so the response time doesn't reveal which usernames exist.
pub fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| hash("dummy-password-for-timing-0").expect("argon2 hashing works"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_only_the_exact_password() {
        let h = hash("Secret123").unwrap();
        assert!(verify("Secret123", &h));
        assert!(!verify("secret123", &h), "passwords are case-sensitive");
        assert!(!verify("Secret1234", &h));
        assert!(!verify("Secret123", "not-a-phc-hash"));
    }
}
