//! Standard Windows save-search roots.

use std::path::PathBuf;

/// The standard Windows save-search roots derived from a user profile dir and
/// the `LocalAppData` dir (both optional). Pure (no environment access) so it
/// is testable; see [`save_search_roots`] for the live version.
pub fn roots_from(user_profile: Option<&str>, local_appdata: Option<&str>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(local) = local_appdata {
        roots.push(PathBuf::from(local));
    }
    if let Some(profile) = user_profile {
        let profile = PathBuf::from(profile);
        roots.push(profile.join("AppData").join("LocalLow"));
        roots.push(profile.join("Documents").join("My Games"));
        roots.push(profile.join("Saved Games"));
    }
    roots
}

/// The live standard Windows save-search roots, read from the environment.
pub fn save_search_roots() -> Vec<PathBuf> {
    let profile = std::env::var("USERPROFILE").ok();
    let local = std::env::var("LOCALAPPDATA").ok();
    roots_from(profile.as_deref(), local.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_search_roots_includes_localappdata_when_set() {
        // Compute against an explicit profile/localappdata so the test is
        // deterministic regardless of the real environment.
        let roots = roots_from(
            Some("C:/Users/Tester"),
            Some("C:/Users/Tester/AppData/Local"),
        );
        assert!(roots
            .iter()
            .any(|p| p == &PathBuf::from("C:/Users/Tester/AppData/Local")));
        assert!(roots
            .iter()
            .any(|p| p == &PathBuf::from("C:/Users/Tester/AppData/LocalLow")));
        assert!(roots.iter().any(|p| p.ends_with("Saved Games")));
        assert!(roots.iter().any(|p| p.ends_with("My Games")));
    }

    #[test]
    fn missing_env_yields_empty() {
        assert!(roots_from(None, None).is_empty());
    }
}
