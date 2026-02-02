use async_trait::async_trait;
use chrono::Utc;
use ironfish_core::{ApiToken, Error, Result, TokenStore};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;
#[derive(Clone)]
pub struct SledTokenStore {
    db: Arc<sled::Db>,
    tokens_tree: sled::Tree,
    hash_index: sled::Tree,
}
impl SledTokenStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let db = sled::open(path).map_err(|e| Error::Storage(e.to_string()))?;
        let tokens_tree = db
            .open_tree("tokens")
            .map_err(|e| Error::Storage(e.to_string()))?;
        let hash_index = db
            .open_tree("token_hashes")
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self {
            db: Arc::new(db),
            tokens_tree,
            hash_index,
        })
    }
    pub fn in_memory() -> Result<Self> {
        let config = sled::Config::new().temporary(true);
        let db = config.open().map_err(|e| Error::Storage(e.to_string()))?;
        let tokens_tree = db
            .open_tree("tokens")
            .map_err(|e| Error::Storage(e.to_string()))?;
        let hash_index = db
            .open_tree("token_hashes")
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self {
            db: Arc::new(db),
            tokens_tree,
            hash_index,
        })
    }
    fn serialize_token(token: &ApiToken) -> Result<Vec<u8>> {
        serde_json::to_vec(token).map_err(Error::Serialization)
    }
    fn deserialize_token(data: &[u8]) -> Result<ApiToken> {
        serde_json::from_slice(data).map_err(Error::Serialization)
    }
}
#[async_trait]
impl TokenStore for SledTokenStore {
    async fn create(&self, token: ApiToken) -> Result<()> {
        let key = token.id.as_bytes().to_vec();
        let data = Self::serialize_token(&token)?;
        self.tokens_tree
            .insert(&key, data)
            .map_err(|e| Error::Storage(e.to_string()))?;
        self.hash_index
            .insert(token.token_hash.as_bytes(), key)
            .map_err(|e| Error::Storage(e.to_string()))?;
        self.db.flush().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }
    async fn get(&self, id: &Uuid) -> Result<Option<ApiToken>> {
        let key = id.as_bytes();
        match self
            .tokens_tree
            .get(key)
            .map_err(|e| Error::Storage(e.to_string()))?
        {
            Some(data) => Ok(Some(Self::deserialize_token(&data)?)),
            None => Ok(None),
        }
    }
    async fn get_by_hash(&self, hash: &str) -> Result<Option<ApiToken>> {
        match self
            .hash_index
            .get(hash.as_bytes())
            .map_err(|e| Error::Storage(e.to_string()))?
        {
            Some(id_bytes) => {
                match self
                    .tokens_tree
                    .get(&id_bytes)
                    .map_err(|e| Error::Storage(e.to_string()))?
                {
                    Some(data) => Ok(Some(Self::deserialize_token(&data)?)),
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }
    async fn update(&self, token: ApiToken) -> Result<()> {
        let key = token.id.as_bytes().to_vec();
        let data = Self::serialize_token(&token)?;
        self.tokens_tree
            .insert(&key, data)
            .map_err(|e| Error::Storage(e.to_string()))?;
        self.db.flush().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }
    async fn delete(&self, id: &Uuid) -> Result<()> {
        if let Some(token) = self.get(id).await? {
            self.hash_index
                .remove(token.token_hash.as_bytes())
                .map_err(|e| Error::Storage(e.to_string()))?;
        }
        self.tokens_tree
            .remove(id.as_bytes())
            .map_err(|e| Error::Storage(e.to_string()))?;
        self.db.flush().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }
    async fn list(&self) -> Result<Vec<ApiToken>> {
        let mut tokens = Vec::new();
        for result in self.tokens_tree.iter() {
            let (_, data) = result.map_err(|e| Error::Storage(e.to_string()))?;
            let token = Self::deserialize_token(&data)?;
            tokens.push(token);
        }
        Ok(tokens)
    }
    async fn revoke(&self, id: &Uuid) -> Result<()> {
        if let Some(mut token) = self.get(id).await? {
            token.revoked = true;
            token.last_used_at = Some(Utc::now());
            self.update(token).await?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::TokenManager;
    use ironfish_core::CreateTokenRequest;
    #[tokio::test]
    async fn test_token_store() {
        let store = SledTokenStore::in_memory().unwrap();
        let secret = TokenManager::generate_secret();
        let manager = TokenManager::new(&secret, "test");
        let (token, _) = manager
            .create(CreateTokenRequest {
                name: Some("test".into()),
                expires_in_days: None,
                rate_limit: None,
            })
            .unwrap();
        store.create(token.clone()).await.unwrap();
        let retrieved = store.get(&token.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, token.id);
        let by_hash = store.get_by_hash(&token.token_hash).await.unwrap().unwrap();
        assert_eq!(by_hash.id, token.id);
        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 1);
        store.revoke(&token.id).await.unwrap();
        let revoked = store.get(&token.id).await.unwrap().unwrap();
        assert!(revoked.revoked);
        store.delete(&token.id).await.unwrap();
        assert!(store.get(&token.id).await.unwrap().is_none());
    }
}
