/// Errors that can occur during cache operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// Serialization or deserialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Backend-specific error.
    #[error("backend error: {0}")]
    Backend(String),

    /// The cache entry has expired.
    #[error("cache entry expired")]
    Expired,

    /// The cache is full and cannot accept new entries.
    #[error("cache full")]
    Full,

    /// A generic cache error.
    #[error("cache error: {0}")]
    Other(String),
}
