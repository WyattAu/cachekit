/// Errors that can occur during cache operations.
///
/// `Cow<'static, str>` variants avoid allocating when the error is a
/// static string slice (e.g. `"redis down"` literal) — the common case for
/// backend error messages.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// Serialization or deserialization error.
    #[error("serialization error: {0}")]
    Serialization(std::borrow::Cow<'static, str>),

    /// Backend-specific error.
    #[error("backend error: {0}")]
    Backend(std::borrow::Cow<'static, str>),

    /// The cache entry has expired.
    #[error("cache entry expired")]
    Expired,

    /// The cache is full and cannot accept new entries.
    #[error("cache full")]
    Full,

    /// A generic cache error.
    #[error("cache error: {0}")]
    Other(std::borrow::Cow<'static, str>),
}
