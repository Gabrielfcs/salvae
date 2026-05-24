//! Convert the upstream Ludusavi manifest (YAML) into Salvaê's compact JSON.
//!
//! Usage: `cargo run --example convert-manifest -p salvae-detect -- <in.yaml> <out.json>`
//!
//! Keeps only what we need to locate Windows save folders: per game, its Steam
//! id and the save-path templates that (a) are tagged as saves (or untagged),
//! (b) apply on Windows, and (c) start with a placeholder our resolver knows.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Placeholders `manifest::Placeholders::resolve` understands.
const SUPPORTED_HEADS: &[&str] = &[
    "<home>",
    "<winLocalAppData>",
    "<winAppData>",
    "<winDocuments>",
    "<base>",
    "<root>",
];

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    files: BTreeMap<String, FileMeta>,
    #[serde(default)]
    steam: Option<Steam>,
}

#[derive(Deserialize)]
struct FileMeta {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    when: Vec<When>,
}

#[derive(Deserialize)]
struct Steam {
    #[serde(default)]
    id: Option<u64>,
}

#[derive(Deserialize)]
struct When {
    #[serde(default)]
    os: Option<String>,
}

#[derive(Serialize)]
struct OutEntry {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    steam_id: Option<u64>,
    paths: Vec<String>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .expect("usage: convert-manifest <in.yaml> <out.json>");
    let output = args
        .next()
        .expect("usage: convert-manifest <in.yaml> <out.json>");

    let yaml = std::fs::read_to_string(&input).expect("read manifest yaml");
    let games: BTreeMap<String, Entry> = serde_yaml::from_str(&yaml).expect("parse manifest yaml");

    let mut out: Vec<OutEntry> = Vec::new();
    for (name, entry) in games {
        let mut paths: Vec<String> = Vec::new();
        for (template, meta) in &entry.files {
            let is_save = meta.tags.is_empty() || meta.tags.iter().any(|t| t == "save");
            if !is_save {
                continue;
            }
            let windows_ok = meta.when.is_empty()
                || meta.when.iter().any(|w| match w.os.as_deref() {
                    Some(os) => os == "windows",
                    None => true,
                });
            if !windows_ok {
                continue;
            }
            let norm = template.replace('\\', "/");
            let head = norm.split('/').next().unwrap_or("");
            if !SUPPORTED_HEADS.contains(&head) {
                continue;
            }
            // Skip templates with placeholders/globs we can't resolve at runtime.
            if norm.contains('*') || norm[head.len()..].contains('<') {
                continue;
            }
            if !paths.contains(template) {
                paths.push(template.clone());
            }
        }
        let steam_id = entry.steam.and_then(|s| s.id);
        if paths.is_empty() {
            continue; // no usable Windows save path
        }
        out.push(OutEntry {
            name,
            steam_id,
            paths,
        });
    }

    let json = serde_json::to_string(&out).expect("serialize json");
    std::fs::write(&output, json).expect("write json");
    eprintln!("wrote {output}: {} games", out.len());
}
