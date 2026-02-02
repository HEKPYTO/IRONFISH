use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Duration, Utc};
use ironfish_core::{ApiToken, CreateTokenRequest, CreateTokenResponse, Error, Result};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use uuid::Uuid;
const TOKEN_PREFIX: &str = "iff_";
const TOKEN_RANDOM_BYTES: usize = 32;
pub struct TokenManager {
    key: hmac::Key,
    node_id: String,
    default_ttl_days: u32,
}
impl TokenManager {
    pub fn new(secret: &[u8], node_id: impl Into<String>) -> Self {
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
        Self {
            key,
            node_id: node_id.into(),
            default_ttl_days: 365,
        }
    }
    pub fn with_default_ttl(mut self, days: u32) -> Self {
        self.default_ttl_days = days;
        self
    }
    pub fn generate_secret() -> Vec<u8> {
        let rng = SystemRandom::new();
        let mut secret = vec![0u8; 32];
        rng.fill(&mut secret).expect("failed to generate secret");
        secret
    }
    pub fn create(&self, request: CreateTokenRequest) -> Result<(ApiToken, CreateTokenResponse)> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let expires_at = request
            .expires_in_days
            .map(|days| now + Duration::days(days as i64))
            .or_else(|| Some(now + Duration::days(self.default_ttl_days as i64)));
        let raw_token = self.generate_raw_token(&id)?;
        let token_hash = self.hash_token(&raw_token);
        let api_token = ApiToken {
            id,
            name: request.name,
            token_hash,
            created_at: now,
            expires_at,
            last_used_at: None,
            created_by_node: self.node_id.clone(),
            revoked: false,
            rate_limit: request.rate_limit,
        };
        let formatted = format!("{}{}", TOKEN_PREFIX, raw_token);
        let response = CreateTokenResponse {
            id,
            token: formatted,
            expires_at,
        };
        Ok((api_token, response))
    }
    fn generate_raw_token(&self, id: &Uuid) -> Result<String> {
        let rng = SystemRandom::new();
        let mut random_bytes = vec![0u8; TOKEN_RANDOM_BYTES];
        rng.fill(&mut random_bytes)
            .map_err(|_| Error::Internal("rng failed".into()))?;
        let timestamp = Utc::now().timestamp();
        let mut data = Vec::new();
        data.extend_from_slice(id.as_bytes());
        data.extend_from_slice(&random_bytes);
        data.extend_from_slice(&timestamp.to_le_bytes());
        let signature = hmac::sign(&self.key, &data);
        data.extend_from_slice(signature.as_ref());
        Ok(URL_SAFE_NO_PAD.encode(&data))
    }
    pub fn hash_token(&self, raw_token: &str) -> String {
        let signature = hmac::sign(&self.key, raw_token.as_bytes());
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    }
    pub fn extract_raw_token(token: &str) -> Option<&str> {
        token.strip_prefix(TOKEN_PREFIX)
    }
    pub fn validate_format(token: &str) -> bool {
        if let Some(raw) = Self::extract_raw_token(token) {
            URL_SAFE_NO_PAD.decode(raw).is_ok()
        } else {
            false
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_token_generation() {
        let secret = TokenManager::generate_secret();
        let manager = TokenManager::new(&secret, "test-node");
        let request = CreateTokenRequest {
            name: Some("test".into()),
            expires_in_days: Some(30),
            rate_limit: None,
        };
        let (token, response) = manager.create(request).unwrap();
        assert!(response.token.starts_with(TOKEN_PREFIX));
        assert!(token.is_valid());
        assert!(TokenManager::validate_format(&response.token));
    }
    #[test]
    fn test_token_hash() {
        let secret = TokenManager::generate_secret();
        let manager = TokenManager::new(&secret, "test-node");
        let raw = "test_token_data";
        let hash1 = manager.hash_token(raw);
        let hash2 = manager.hash_token(raw);
        assert_eq!(hash1, hash2);
    }
}
