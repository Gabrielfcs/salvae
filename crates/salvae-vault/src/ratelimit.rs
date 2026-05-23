//! Pure rate-limit retry-delay decision.
//!
//! Given an HTTP status and a way to read response headers, decide how many
//! seconds to wait before retrying. No IO, no sleeping — the caller sleeps.

/// Default backoff (seconds) when a 429 has no usable timing header.
pub const DEFAULT_BACKOFF_SECS: f64 = 1.0;

/// Decide the retry delay in seconds, or `None` if the request should not be
/// retried (any non-429 status). Prefers the `Retry-After` header, then
/// `X-RateLimit-Reset-After`, then a default backoff.
///
/// `header` is a lookup returning the value of a (lowercased) header name.
pub fn retry_after_secs(status: u16, header: impl Fn(&str) -> Option<String>) -> Option<f64> {
    if status != 429 {
        return None;
    }
    if let Some(secs) = header("retry-after").and_then(|v| v.trim().parse::<f64>().ok()) {
        return Some(secs.max(0.0));
    }
    if let Some(secs) = header("x-ratelimit-reset-after").and_then(|v| v.trim().parse::<f64>().ok())
    {
        return Some(secs.max(0.0));
    }
    Some(DEFAULT_BACKOFF_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_map<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn non_429_does_not_retry() {
        assert_eq!(retry_after_secs(200, header_map(&[])), None);
        assert_eq!(retry_after_secs(500, header_map(&[])), None);
    }

    #[test]
    fn prefers_retry_after_header() {
        let h = header_map(&[("retry-after", "2.5"), ("x-ratelimit-reset-after", "9")]);
        assert_eq!(retry_after_secs(429, h), Some(2.5));
    }

    #[test]
    fn falls_back_to_reset_after() {
        let h = header_map(&[("x-ratelimit-reset-after", "1.25")]);
        assert_eq!(retry_after_secs(429, h), Some(1.25));
    }

    #[test]
    fn falls_back_to_default_when_no_headers() {
        assert_eq!(
            retry_after_secs(429, header_map(&[])),
            Some(DEFAULT_BACKOFF_SECS)
        );
    }

    #[test]
    fn negative_values_are_clamped_to_zero() {
        let h = header_map(&[("retry-after", "-3")]);
        assert_eq!(retry_after_secs(429, h), Some(0.0));
    }
}
