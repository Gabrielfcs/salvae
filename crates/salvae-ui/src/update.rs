//! Self-update: check GitHub Releases for a newer version, download and verify
//! the new installer, and launch it silently. The installer (see
//! `packaging/installer.iss`) closes this app, replaces the files, and reopens
//! it. Pure logic (release/version/checksum parsing) is unit-tested; HTTP and
//! process launching are thin wrappers verified manually.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// GitHub repository the releases come from.
const LATEST_URL: &str = "https://api.github.com/repos/Gabrielfcs/salvae/releases/latest";
/// Asset names published on each release (a contract with `scripts/release.ps1`).
const SETUP_ASSET: &str = "Salvae-Setup.exe";
const CHECKSUM_ASSET: &str = "Salvae-Setup.exe.b3";
/// GitHub requires a User-Agent on API requests.
const USER_AGENT: &str = concat!("Salvae/", env!("CARGO_PKG_VERSION"));

/// A newer release than the running version, with the URLs to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: semver::Version,
    pub setup_url: String,
    pub checksum_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Parse GitHub's `releases/latest` JSON. Returns `Some` only when the release
/// tag is a valid semver strictly greater than `current` AND both the setup and
/// checksum assets are present.
pub fn parse_latest_release(json: &str, current: &semver::Version) -> Option<AvailableUpdate> {
    let release: Release = serde_json::from_str(json).ok()?;
    let tag = release.tag_name.trim().trim_start_matches('v');
    let version = semver::Version::parse(tag).ok()?;
    if version <= *current {
        return None;
    }
    let url_of = |wanted: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == wanted)
            .map(|a| a.browser_download_url.clone())
    };
    Some(AvailableUpdate {
        version,
        setup_url: url_of(SETUP_ASSET)?,
        checksum_url: url_of(CHECKSUM_ASSET)?,
    })
}

/// Extract the hex digest from a `b3sum`-style checksum file ("<hex>  <name>").
pub fn checksum_hex(contents: &str) -> &str {
    contents.split_whitespace().next().unwrap_or("")
}

/// Check that `bytes` hash to `expected_hex` (blake3, case-insensitive).
pub fn verify_blake3(bytes: &[u8], expected_hex: &str) -> bool {
    blake3::hash(bytes)
        .to_hex()
        .as_str()
        .eq_ignore_ascii_case(expected_hex.trim())
}

fn http_get_string(url: &str) -> Result<String, String> {
    ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Ask GitHub for the latest release and return it if it is newer than
/// `current`. Returns `None` on any network/parse failure (never errors out —
/// the caller just tries again later).
pub fn check(current: &semver::Version) -> Option<AvailableUpdate> {
    let json = http_get_string(LATEST_URL).ok()?;
    parse_latest_release(&json, current)
}

/// Download the installer and its checksum, verify the blake3, and write the
/// installer to a temp file. Returns the path to run.
pub fn download_and_verify(update: &AvailableUpdate) -> Result<PathBuf, String> {
    let setup = http_get_bytes(&update.setup_url)?;
    let checksum = http_get_string(&update.checksum_url)?;
    if !verify_blake3(&setup, checksum_hex(&checksum)) {
        return Err("checksum do instalador não confere".into());
    }
    let dir = std::env::temp_dir().join("salvae-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(SETUP_ASSET);
    std::fs::write(&path, &setup).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Launch the installer silently. It (via `AppMutex` + Restart Manager) closes
/// this running app, replaces the files, and relaunches it. We do NOT exit
/// ourselves: if the user denies UAC, the installer aborts and we keep running.
pub fn launch_installer(setup_path: &Path) -> Result<(), String> {
    std::process::Command::new(setup_path)
        .args(["/VERYSILENT", "/NORESTART"])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ver(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn release_json(tag: &str, with_assets: bool) -> String {
        let assets = if with_assets {
            r#"[
                {"name":"Salvae-Setup.exe","browser_download_url":"https://example/Setup.exe"},
                {"name":"Salvae-Setup.exe.b3","browser_download_url":"https://example/Setup.b3"}
            ]"#
        } else {
            "[]"
        };
        format!(r#"{{"tag_name":"{tag}","assets":{assets}}}"#)
    }

    #[test]
    fn newer_release_with_assets_parses() {
        let json = release_json("v1.2.0", true);
        let up = parse_latest_release(&json, &ver("1.1.1")).unwrap();
        assert_eq!(up.version, ver("1.2.0"));
        assert_eq!(up.setup_url, "https://example/Setup.exe");
        assert_eq!(up.checksum_url, "https://example/Setup.b3");
    }

    #[test]
    fn same_or_older_release_is_none() {
        assert!(parse_latest_release(&release_json("v1.1.1", true), &ver("1.1.1")).is_none());
        assert!(parse_latest_release(&release_json("v1.0.0", true), &ver("1.1.1")).is_none());
    }

    #[test]
    fn newer_release_without_assets_is_none() {
        assert!(parse_latest_release(&release_json("v2.0.0", false), &ver("1.1.1")).is_none());
    }

    #[test]
    fn bad_tag_or_json_is_none() {
        assert!(parse_latest_release(&release_json("nightly", true), &ver("1.1.1")).is_none());
        assert!(parse_latest_release("not json", &ver("1.1.1")).is_none());
    }

    #[test]
    fn checksum_hex_takes_the_first_token() {
        assert_eq!(checksum_hex("abc123  Salvae-Setup.exe\n"), "abc123");
        assert_eq!(checksum_hex("deadbeef"), "deadbeef");
        assert_eq!(checksum_hex("   "), "");
    }

    #[test]
    fn verify_blake3_matches_only_the_right_hash() {
        let data = b"installer bytes";
        let right = blake3::hash(data).to_hex().to_string();
        assert!(verify_blake3(data, &right));
        assert!(verify_blake3(data, &right.to_uppercase())); // case-insensitive
        assert!(!verify_blake3(data, "00")); // wrong
        assert!(!verify_blake3(b"tampered", &right)); // wrong bytes
    }
}
