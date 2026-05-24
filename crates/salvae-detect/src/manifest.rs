//! Ludusavi-derived map of games to their save-path templates — the
//! highest-accuracy layer of save-folder detection.
//!
//! Attribution: the embedded data is derived from the Ludusavi manifest
//! (github.com/mtkennerly/ludusavi-manifest, MIT), which aggregates data from
//! PCGamingWiki (CC BY-NC-SA). See `assets/CREDITS-ludusavi.md`.

use std::path::PathBuf;

use serde::Deserialize;

use crate::DetectError;

/// One game's save-path templates, keyed by Steam id and/or name.
#[derive(Debug, Clone, Deserialize)]
pub struct SaveEntry {
    pub name: String,
    #[serde(default)]
    pub steam_id: Option<u64>,
    #[serde(default)]
    pub paths: Vec<String>,
}

/// The curated save-location dataset.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    entries: Vec<SaveEntry>,
}

impl Manifest {
    /// Parse the embedded curated manifest (empty on any parse error).
    pub fn embedded() -> Self {
        Self::from_json(include_str!("../assets/ludusavi-manifest.json")).unwrap_or_default()
    }

    /// Parse a manifest from JSON (a top-level array of entries).
    pub fn from_json(json: &str) -> Result<Self, DetectError> {
        let entries = serde_json::from_str(json).map_err(|e| DetectError::Parse(e.to_string()))?;
        Ok(Self { entries })
    }

    /// Path templates for a game — matched by Steam id first, then by name.
    pub fn paths_for(&self, steam_id: Option<u64>, name: &str) -> Vec<String> {
        if let Some(id) = steam_id {
            if let Some(entry) = self.entries.iter().find(|e| e.steam_id == Some(id)) {
                return entry.paths.clone();
            }
        }
        let needle = normalize(name);
        self.entries
            .iter()
            .find(|e| normalize(&e.name) == needle)
            .map(|e| e.paths.clone())
            .unwrap_or_default()
    }
}

/// Base directories used to resolve manifest path placeholders.
#[derive(Debug, Clone, Default)]
pub struct Placeholders {
    pub home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub install_dir: Option<PathBuf>,
}

impl Placeholders {
    /// Resolve placeholders from the live Windows environment.
    pub fn live(install_dir: Option<PathBuf>) -> Self {
        let home = std::env::var("USERPROFILE").ok().map(PathBuf::from);
        Self {
            documents: home.as_ref().map(|h| h.join("Documents")),
            home,
            local_app_data: std::env::var("LOCALAPPDATA").ok().map(PathBuf::from),
            app_data: std::env::var("APPDATA").ok().map(PathBuf::from),
            install_dir,
        }
    }

    /// Resolve a single template to an absolute path, or `None` if it begins
    /// with (or still contains) a placeholder we don't understand.
    pub fn resolve(&self, template: &str) -> Option<PathBuf> {
        let norm = template.replace('\\', "/");
        let (head, rest) = match norm.split_once('/') {
            Some((h, r)) => (h, Some(r)),
            None => (norm.as_str(), None),
        };
        let base = match head {
            "<home>" => self.home.clone()?,
            "<winLocalAppData>" => self.local_app_data.clone()?,
            "<winAppData>" => self.app_data.clone()?,
            "<winDocuments>" => self.documents.clone()?,
            "<base>" | "<root>" => self.install_dir.clone()?,
            _ => return None,
        };
        let mut path = base;
        if let Some(rest) = rest {
            // Bail on any placeholder we can't resolve (e.g. <storeUserId>).
            if rest.contains('<') {
                return None;
            }
            for comp in rest.split('/') {
                path.push(comp);
            }
        }
        Some(path)
    }
}

/// Lowercase + strip non-alphanumerics, for fuzzy name comparison.
pub(crate) fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_parses_and_matches_by_steam_id() {
        let m = Manifest::embedded();
        let paths = m.paths_for(Some(2709570), "anything");
        assert_eq!(
            paths,
            vec!["<home>/AppData/LocalLow/DDTNL/Supermarket Together"]
        );
    }

    #[test]
    fn matches_by_name_when_no_steam_id() {
        let m = Manifest::embedded();
        assert!(!m.paths_for(None, "valheim").is_empty());
        assert!(m.paths_for(None, "no such game").is_empty());
    }

    #[test]
    fn resolves_known_placeholders() {
        let ph = Placeholders {
            home: Some(PathBuf::from("C:/Users/Me")),
            local_app_data: Some(PathBuf::from("C:/Users/Me/AppData/Local")),
            ..Default::default()
        };
        assert_eq!(
            ph.resolve("<home>/AppData/LocalLow/DDTNL/Supermarket Together"),
            Some(PathBuf::from(
                "C:/Users/Me/AppData/LocalLow/DDTNL/Supermarket Together"
            ))
        );
        assert_eq!(
            ph.resolve("<winLocalAppData>/Pal/Saved"),
            Some(PathBuf::from("C:/Users/Me/AppData/Local/Pal/Saved"))
        );
    }

    #[test]
    fn rejects_unknown_or_remaining_placeholders() {
        let ph = Placeholders {
            home: Some(PathBuf::from("C:/Users/Me")),
            ..Default::default()
        };
        assert_eq!(ph.resolve("<winSavedGames>/x"), None); // unknown head
        assert_eq!(ph.resolve("<home>/Steam/<storeUserId>/remote"), None); // leftover
        assert_eq!(ph.resolve("<winDocuments>/x"), None); // missing base
    }
}
