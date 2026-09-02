use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{CacheBackend, CacheEntry, CacheStats};

/// SQLite-backed cache backend.
pub struct SqliteBackend {
    conn: Mutex<Connection>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl SqliteBackend {
    /// Create a new SQLite backend, opening (or creating) the database at `path`.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cache (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL,
                created_at_secs INTEGER NOT NULL,
                created_at_nanos INTEGER NOT NULL,
                expires_at_secs INTEGER,
                expires_at_nanos INTEGER,
                max_age_secs INTEGER,
                max_age_nanos INTEGER,
                stale_until_secs INTEGER,
                stale_until_nanos INTEGER
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        })
    }

    /// Create a new SQLite backend using an in-memory database.
    pub fn in_memory() -> Result<Self, rusqlite::Error> {
        Self::new(":memory:")
    }

    fn system_time_to_parts(t: SystemTime) -> (i64, u32) {
        let dur = t.duration_since(UNIX_EPOCH).unwrap_or_default();
        (dur.as_secs() as i64, dur.subsec_nanos())
    }

    fn parts_to_system_time(secs: i64, nanos: u32) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs as u64) + Duration::from_nanos(nanos as u64)
    }

    fn sys_to_instant(sys: SystemTime) -> std::time::Instant {
        let now_instant = std::time::Instant::now();
        let now_sys = SystemTime::now();
        match now_sys.duration_since(sys) {
            Ok(elapsed) => now_instant.checked_sub(elapsed).unwrap_or(now_instant),
            Err(e) => {
                let future = e.duration();
                now_instant.checked_add(future).unwrap_or(now_instant)
            }
        }
    }
}

#[async_trait::async_trait]
impl CacheBackend for SqliteBackend {
    type K = String;
    type V = Vec<u8>;

    async fn get(&self, key: &String) -> Result<Option<CacheEntry<Vec<u8>>>, crate::CacheError> {
        let result = {
            let conn = self.conn.lock().map_err(|e| crate::CacheError::Backend(e.to_string()))?;
            conn.query_row(
                "SELECT value, created_at_secs, created_at_nanos,
                        expires_at_secs, expires_at_nanos,
                        max_age_secs, max_age_nanos,
                        stale_until_secs, stale_until_nanos
                 FROM cache WHERE key = ?1",
                params![key],
                |row| {
                    let value: Vec<u8> = row.get(0)?;
                    let created_at_secs: i64 = row.get(1)?;
                    let created_at_nanos: u32 = row.get(2)?;
                    let expires_at_secs: Option<i64> = row.get(3)?;
                    let expires_at_nanos: Option<u32> = row.get(4)?;
                    let max_age_secs: Option<i64> = row.get(5)?;
                    let max_age_nanos: Option<u32> = row.get(6)?;
                    let stale_until_secs: Option<i64> = row.get(7)?;
                    let stale_until_nanos: Option<u32> = row.get(8)?;
                    Ok((
                        value,
                        created_at_secs,
                        created_at_nanos,
                        expires_at_secs,
                        expires_at_nanos,
                        max_age_secs,
                        max_age_nanos,
                        stale_until_secs,
                        stale_until_nanos,
                    ))
                },
            )
        };

        match result {
            Ok((
                value,
                cas,
                can,
                eas,
                ean,
                mas,
                man,
                sus,
                sun,
            )) => {
                let created_at = Self::sys_to_instant(Self::parts_to_system_time(cas, can));
                let expires_at = eas.map(|s| {
                    Self::sys_to_instant(Self::parts_to_system_time(s, ean.unwrap_or(0)))
                });
                let max_age_at = mas.map(|s| {
                    Self::sys_to_instant(Self::parts_to_system_time(s, man.unwrap_or(0)))
                });
                let stale_until = sus.map(|s| {
                    Self::sys_to_instant(Self::parts_to_system_time(s, sun.unwrap_or(0)))
                });

                if let Some(exp) = expires_at {
                    if std::time::Instant::now() >= exp {
                        return Ok(None);
                    }
                }

                self.hits.fetch_add(1, Ordering::Relaxed);
                Ok(Some(CacheEntry {
                    value,
                    created_at,
                    expires_at,
                    max_age_at,
                    stale_until,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                Ok(None)
            }
            Err(e) => Err(crate::CacheError::Backend(e.to_string())),
        }
    }

    async fn insert(&self, key: String, value: Vec<u8>) -> Result<(), crate::CacheError> {
        let conn = self.conn.lock().map_err(|e| crate::CacheError::Backend(e.to_string()))?;
        let (cas, can) = Self::system_time_to_parts(SystemTime::now());
        conn.execute(
            "INSERT OR REPLACE INTO cache (key, value, created_at_secs, created_at_nanos)
             VALUES (?1, ?2, ?3, ?4)",
            params![key, value, cas, can],
        )
        .map_err(|e| crate::CacheError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn insert_with_swr(
        &self,
        key: String,
        value: Vec<u8>,
        max_age: Duration,
        stale_while_revalidate: Duration,
    ) -> Result<(), crate::CacheError> {
        let conn = self.conn.lock().map_err(|e| crate::CacheError::Backend(e.to_string()))?;
        let now = SystemTime::now();
        let (cas, can) = Self::system_time_to_parts(now);

        let total_ttl = max_age + stale_while_revalidate;
        let exp = now + total_ttl;
        let ma = now + max_age;
        let su = now + total_ttl;

        let (eas, ean) = Self::system_time_to_parts(exp);
        let (mas, man) = Self::system_time_to_parts(ma);
        let (sus, sun) = Self::system_time_to_parts(su);

        conn.execute(
            "INSERT OR REPLACE INTO cache
             (key, value, created_at_secs, created_at_nanos,
              expires_at_secs, expires_at_nanos,
              max_age_secs, max_age_nanos,
              stale_until_secs, stale_until_nanos)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![key, value, cas, can, eas, ean, mas, man, sus, sun],
        )
        .map_err(|e| crate::CacheError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn remove(&self, key: &String) -> Result<Option<Vec<u8>>, crate::CacheError> {
        let conn = self.conn.lock().map_err(|e| crate::CacheError::Backend(e.to_string()))?;

        let value: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value FROM cache WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| crate::CacheError::Backend(e.to_string()))?;

        if value.is_some() {
            conn.execute("DELETE FROM cache WHERE key = ?1", params![key])
                .map_err(|e| crate::CacheError::Backend(e.to_string()))?;
        }

        Ok(value)
    }

    async fn clear(&self) -> Result<(), crate::CacheError> {
        let conn = self.conn.lock().map_err(|e| crate::CacheError::Backend(e.to_string()))?;
        conn.execute("DELETE FROM cache", [])
            .map_err(|e| crate::CacheError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn stats(&self) -> Result<CacheStats, crate::CacheError> {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };

        let conn = self.conn.lock().map_err(|e| crate::CacheError::Backend(e.to_string()))?;

        let now = SystemTime::now();
        let (secs, _nanos) = Self::system_time_to_parts(now);

        let size: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cache WHERE expires_at_secs IS NULL OR expires_at_secs > ?1",
                params![secs],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(CacheStats {
            hits,
            misses,
            hit_rate,
            size: size as u64,
        })
    }
}
