//! Run a `ureq` request with rate-limit-aware retries.
//!
//! On HTTP 429 we sleep (per [`salvae_vault::ratelimit::retry_after_secs`]) and
//! retry up to `max_retries` times. 404 maps to `NotFound`; other failures map
//! to `Transport`. The sleep function is injected so tests run instantly.

use salvae_vault::ratelimit::retry_after_secs;
use salvae_vault::VaultError;

/// Execute `call` (a `ureq` request) with rate-limit-aware retries.
///
/// - `max_retries`: how many additional attempts to make after the first.
/// - `sleep`: invoked with a number of seconds before each retry (inject a
///   no-op in tests; production passes a real sleep).
/// - `call`: builds and performs the request, returning the `ureq` result.
pub fn execute_with_retry<F, S>(
    max_retries: u32,
    sleep: S,
    mut call: F,
) -> Result<ureq::Response, VaultError>
where
    F: FnMut() -> Result<ureq::Response, ureq::Error>,
    S: Fn(f64),
{
    let mut attempts = 0u32;
    loop {
        match call() {
            Ok(resp) => return Ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                let delay = retry_after_secs(code, |h| resp.header(h).map(str::to_string));
                match delay {
                    Some(secs) if attempts < max_retries => {
                        attempts += 1;
                        sleep(secs);
                        continue;
                    }
                    _ => {
                        if code == 404 {
                            return Err(VaultError::NotFound);
                        }
                        return Err(VaultError::Transport(format!("Discord returned HTTP {code}")));
                    }
                }
            }
            Err(e) => return Err(VaultError::Transport(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ureq::{Error, Response};

    fn status_err(code: u16) -> Error {
        Error::Status(code, Response::new(code, "status", "").unwrap())
    }

    #[test]
    fn returns_ok_on_first_success() {
        let mut calls = 0;
        let r = execute_with_retry(3, |_| {}, || {
            calls += 1;
            Ok(Response::new(200, "OK", "{}").unwrap())
        });
        assert!(r.is_ok());
        assert_eq!(calls, 1);
    }

    #[test]
    fn retries_on_429_then_succeeds() {
        let mut calls = 0;
        let r = execute_with_retry(3, |_| {}, || {
            calls += 1;
            if calls == 1 {
                Err(status_err(429))
            } else {
                Ok(Response::new(200, "OK", "{}").unwrap())
            }
        });
        assert!(r.is_ok());
        assert_eq!(calls, 2);
    }

    #[test]
    fn gives_up_after_max_retries_on_persistent_429() {
        let mut calls = 0;
        let r = execute_with_retry(2, |_| {}, || {
            calls += 1;
            Err(status_err(429))
        });
        assert!(matches!(r, Err(VaultError::Transport(_))));
        assert_eq!(calls, 3); // initial attempt + 2 retries
    }

    #[test]
    fn maps_404_to_not_found_without_retry() {
        let mut calls = 0;
        let r = execute_with_retry(3, |_| {}, || {
            calls += 1;
            Err(status_err(404))
        });
        assert!(matches!(r, Err(VaultError::NotFound)));
        assert_eq!(calls, 1);
    }

    #[test]
    fn maps_other_status_to_transport_without_retry() {
        let mut calls = 0;
        let r = execute_with_retry(3, |_| {}, || {
            calls += 1;
            Err(status_err(500))
        });
        assert!(matches!(r, Err(VaultError::Transport(_))));
        assert_eq!(calls, 1);
    }
}
