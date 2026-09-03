use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redis::aio::MultiplexedConnection;

use crate::{CacheBackend, CacheEntry, CacheError, CacheStats};

/// Redis cache backend using `redis` crate.
///
/// `MultiplexedConnection` is `Clone` via an internal `Arc`; each
/// `self.conn.clone()` in the `CacheBackend` impl is therefore an atomic
/// refcount bump, not a new TCP connection. The trait requires `&self`, so
/// we must clone to obtain a `&mut` handle for `query_async`. Alternative
/// would be `Arc<Mutex<Connection>>` which would serialize all commands.
pub struct RedisBackend<K, V> {
    conn: MultiplexedConnection,
    prefix: String,
    hits: AtomicU64,
    misses: AtomicU64,
    _phantom: std::marker::PhantomData<(K, V)>,
}

impl<K, V> RedisBackend<K, V>
where
    K: std::fmt::Display + Send + Sync + 'static,
    V: serde::Serialize + serde::de::DeserializeOwned + Clone + Send + Sync + 'static,
{
    /// Create a new Redis backend from an existing connection.
    pub fn new(conn: MultiplexedConnection, prefix: impl Into<String>) -> Self {
        Self {
            conn,
            prefix: prefix.into(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Connect to a Redis URL and create a new backend.
    pub async fn connect(url: &str, prefix: impl Into<String>) -> Result<Self, CacheError> {
        let client =
            redis::Client::open(url).map_err(|e| CacheError::Backend(e.to_string().into()))?;
        let conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;
        Ok(Self::new(conn, prefix))
    }

    fn key_for(&self, key: &K) -> String {
        format!("{}:{}", self.prefix, key)
    }

    fn meta_key_for(&self, key: &K) -> String {
        format!("{}:meta:{}", self.prefix, key)
    }
}

/// Stored metadata for a Redis cache entry.
#[derive(serde::Serialize, serde::Deserialize)]
struct RedisEntryMeta {
    created_at_secs: u64,
    created_at_nanos: u32,
    expires_at_secs: Option<u64>,
    expires_at_nanos: Option<u32>,
    max_age_secs: Option<u64>,
    max_age_nanos: Option<u32>,
    stale_until_secs: Option<u64>,
    stale_until_nanos: Option<u32>,
}

#[async_trait::async_trait]
impl<K, V> CacheBackend for RedisBackend<K, V>
where
    K: std::fmt::Display + Send + Sync + 'static,
    V: serde::Serialize + serde::de::DeserializeOwned + Clone + Send + Sync + 'static,
{
    type K = K;
    type V = V;

    async fn get(&self, key: &K) -> Result<Option<CacheEntry<V>>, CacheError> {
        let redis_key = self.key_for(key);
        let meta_key = self.meta_key_for(key);

        let mut conn = self.conn.clone();

        let (value_json, meta_json): (Option<String>, Option<String>) = redis::cmd("MGET")
            .arg(&redis_key)
            .arg(&meta_key)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;

        let value_json = match value_json {
            Some(v) => v,
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                return Ok(None);
            }
        };

        let value: V = serde_json::from_str(&value_json)
            .map_err(|e| CacheError::Serialization(e.to_string().into()))?;

        let entry = match meta_json {
            Some(mj) => {
                let meta: RedisEntryMeta = serde_json::from_str(&mj)
                    .map_err(|e| CacheError::Serialization(e.to_string().into()))?;

                let created_at = SystemTime::UNIX_EPOCH
                    + Duration::from_secs(meta.created_at_secs)
                    + Duration::from_nanos(meta.created_at_nanos as u64);

                let expires_at = meta.expires_at_secs.map(|s| {
                    SystemTime::UNIX_EPOCH
                        + Duration::from_secs(s)
                        + Duration::from_nanos(meta.expires_at_nanos.unwrap_or(0) as u64)
                });

                let max_age_at = meta.max_age_secs.map(|s| {
                    SystemTime::UNIX_EPOCH
                        + Duration::from_secs(s)
                        + Duration::from_nanos(meta.max_age_nanos.unwrap_or(0) as u64)
                });

                let stale_until = meta.stale_until_secs.map(|s| {
                    SystemTime::UNIX_EPOCH
                        + Duration::from_secs(s)
                        + Duration::from_nanos(meta.stale_until_nanos.unwrap_or(0) as u64)
                });

                // Convert SystemTime -> Instant for the entry
                let now = std::time::Instant::now();
                let sys_now = SystemTime::now();

                let created_at_instant = now
                    .checked_sub(sys_now.duration_since(created_at).unwrap_or_default())
                    .unwrap_or(now);

                let expires_at_instant = expires_at.map(|et| {
                    now.checked_sub(sys_now.duration_since(et).unwrap_or_default())
                        .unwrap_or(now)
                });

                let max_age_at_instant = max_age_at.map(|ma| {
                    now.checked_sub(sys_now.duration_since(ma).unwrap_or_default())
                        .unwrap_or(now)
                });

                let stale_until_instant = stale_until.map(|su| {
                    now.checked_sub(sys_now.duration_since(su).unwrap_or_default())
                        .unwrap_or(now)
                });

                CacheEntry {
                    value,
                    created_at: created_at_instant,
                    expires_at: expires_at_instant,
                    max_age_at: max_age_at_instant,
                    stale_until: stale_until_instant,
                }
            }
            None => CacheEntry {
                value,
                created_at: std::time::Instant::now(),
                expires_at: None,
                max_age_at: None,
                stale_until: None,
            },
        };

        self.hits.fetch_add(1, Ordering::Relaxed);
        Ok(Some(entry))
    }

    async fn insert(&self, key: K, value: V) -> Result<(), CacheError> {
        let redis_key = self.key_for(&key);
        let meta_key = self.meta_key_for(&key);

        let value_json = serde_json::to_string(&value)
            .map_err(|e| CacheError::Serialization(e.to_string().into()))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();

        let meta = RedisEntryMeta {
            created_at_secs: now.as_secs(),
            created_at_nanos: now.subsec_nanos(),
            expires_at_secs: None,
            expires_at_nanos: None,
            max_age_secs: None,
            max_age_nanos: None,
            stale_until_secs: None,
            stale_until_nanos: None,
        };
        let meta_json = serde_json::to_string(&meta)
            .map_err(|e| CacheError::Serialization(e.to_string().into()))?;

        let mut conn = self.conn.clone();
        let _: () = redis::cmd("MSET")
            .arg(&redis_key)
            .arg(&value_json)
            .arg(&meta_key)
            .arg(&meta_json)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;

        Ok(())
    }

    async fn insert_with_swr(
        &self,
        key: K,
        value: V,
        max_age: Duration,
        stale_while_revalidate: Duration,
    ) -> Result<(), CacheError> {
        let redis_key = self.key_for(&key);
        let meta_key = self.meta_key_for(&key);

        let value_json = serde_json::to_string(&value)
            .map_err(|e| CacheError::Serialization(e.to_string().into()))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();

        let total_ttl = max_age + stale_while_revalidate;

        let meta = RedisEntryMeta {
            created_at_secs: now.as_secs(),
            created_at_nanos: now.subsec_nanos(),
            expires_at_secs: Some(now.as_secs() + total_ttl.as_secs()),
            expires_at_nanos: Some(now.subsec_nanos()),
            max_age_secs: Some(now.as_secs() + max_age.as_secs()),
            max_age_nanos: Some(now.subsec_nanos()),
            stale_until_secs: Some(now.as_secs() + total_ttl.as_secs()),
            stale_until_nanos: Some(now.subsec_nanos()),
        };
        let meta_json = serde_json::to_string(&meta)
            .map_err(|e| CacheError::Serialization(e.to_string().into()))?;

        let mut conn = self.conn.clone();
        let _: () = redis::cmd("MSET")
            .arg(&redis_key)
            .arg(&value_json)
            .arg(&meta_key)
            .arg(&meta_json)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;

        // Set TTL on both keys so Redis cleans them up automatically
        let ttl_secs = total_ttl.as_secs();
        let _: () = redis::cmd("EXPIRE")
            .arg(&redis_key)
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;
        let _: () = redis::cmd("EXPIRE")
            .arg(&meta_key)
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;

        Ok(())
    }

    async fn remove(&self, key: &K) -> Result<Option<V>, CacheError> {
        let redis_key = self.key_for(key);
        let meta_key = self.meta_key_for(key);

        let mut conn = self.conn.clone();

        let value_json: Option<String> = redis::cmd("GET")
            .arg(&redis_key)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;

        if let Some(json) = value_json {
            redis::cmd("DEL")
                .arg(&redis_key)
                .arg(&meta_key)
                .query_async::<_, ()>(&mut conn)
                .await
                .map_err(|e| CacheError::Backend(e.to_string().into()))?;

            let value: V = serde_json::from_str(&json)
                .map_err(|e| CacheError::Serialization(e.to_string().into()))?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    async fn clear(&self) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let pattern = format!("{}:*", self.prefix);

        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;

        if !keys.is_empty() {
            redis::cmd("DEL")
                .arg(&keys)
                .query_async::<_, ()>(&mut conn)
                .await
                .map_err(|e| CacheError::Backend(e.to_string().into()))?;
        }

        Ok(())
    }

    async fn stats(&self) -> Result<CacheStats, CacheError> {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };

        let mut conn = self.conn.clone();
        let pattern = format!("{}:*", self.prefix);

        // Filter to only keys with our prefix for accurate count
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string().into()))?;

        Ok(CacheStats {
            hits,
            misses,
            hit_rate,
            size: keys.len() as u64,
        })
    }
}
