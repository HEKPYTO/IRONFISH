use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: Uuid,
    pub name: Option<String>,
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_by_node: String,
    pub revoked: bool,
    pub rate_limit: Option<u32>,
}

impl ApiToken {
    pub fn is_valid(&self) -> bool {
        if self.revoked {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
    pub id: Uuid,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

impl From<&ApiToken> for TokenMetadata {
    fn from(token: &ApiToken) -> Self {
        Self {
            id: token.id,
            name: token.name.clone(),
            created_at: token.created_at,
            expires_at: token.expires_at,
            last_used_at: token.last_used_at,
            revoked: token.revoked,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTokenRequest {
    pub name: Option<String>,
    pub expires_in_days: Option<u32>,
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTokenResponse {
    pub id: Uuid,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
}
