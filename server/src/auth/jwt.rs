use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Access-token claims. The role is included for the client's convenience only;
/// the server re-reads role and is_active from the database on every request.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub school_id: Uuid,
    pub role: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Clone)]
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    ttl: Duration,
}

impl JwtKeys {
    pub fn new(secret: &str, ttl_minutes: i64) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            ttl: Duration::minutes(ttl_minutes),
        }
    }

    /// Issues a signed HS256 token. Returns the token and its expiry time.
    pub fn issue(
        &self,
        user_id: Uuid,
        school_id: Uuid,
        role: &str,
    ) -> anyhow::Result<(String, DateTime<Utc>)> {
        let now = Utc::now();
        let exp = now + self.ttl;
        let claims = Claims {
            sub: user_id,
            school_id,
            role: role.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
        };
        let token = jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)?;
        Ok((token, exp))
    }

    /// Verifies signature, algorithm and expiry (no leeway).
    pub fn verify(&self, token: &str) -> Option<Claims> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &validation)
            .ok()
            .map(|d| d.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-test-secret-test-secret!!";

    #[test]
    fn round_trip() {
        let keys = JwtKeys::new(SECRET, 60);
        let (user, school) = (Uuid::new_v4(), Uuid::new_v4());
        let (token, _) = keys.issue(user, school, "staff").unwrap();
        let claims = keys.verify(&token).unwrap();
        assert_eq!(claims.sub, user);
        assert_eq!(claims.school_id, school);
        assert_eq!(claims.role, "staff");
    }

    #[test]
    fn rejects_wrong_secret_and_tampering() {
        let keys = JwtKeys::new(SECRET, 60);
        let (token, _) = keys.issue(Uuid::new_v4(), Uuid::new_v4(), "staff").unwrap();
        let other = JwtKeys::new("another-secret-another-secret-another", 60);
        assert!(other.verify(&token).is_none());
        let mut tampered = token.clone();
        tampered.push('x');
        assert!(keys.verify(&tampered).is_none());
    }

    #[test]
    fn rejects_expired() {
        let keys = JwtKeys::new(SECRET, -1);
        let (token, _) = keys.issue(Uuid::new_v4(), Uuid::new_v4(), "admin").unwrap();
        assert!(keys.verify(&token).is_none());
    }
}
