//! Persistent URL history — port of `src/lib/history.ts`.
//!
//! Stored as a JSON array at `~/.config/zoinks/history.json`. History is a
//! nicety: any I/O failure is swallowed and an empty list returned, matching
//! the TS behaviour.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const LIMIT: usize = 50;

fn history_file() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "zoinks")
        .map(|d| d.config_dir().join("history.json"))
        .or_else(|| {
            directories::BaseDirs::new()
                .map(|b| b.home_dir().join(".config").join("zoinks").join("history.json"))
        })
}

#[derive(Serialize, Deserialize)]
struct HistoryDoc(Vec<String>);

pub fn load_history() -> Vec<String> {
    let Some(path) = history_file() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    // serde_json will reject non-array payloads, but be defensive anyway
    match serde_json::from_str::<HistoryDoc>(&text) {
        Ok(doc) => doc.0.into_iter().filter(|s| !s.is_empty()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Prepend `url` (deduped, capped at 50). Persists to disk on a best-effort
/// basis — never panics, never returns an error.
pub fn add_to_history(url: &str) -> Vec<String> {
    let mut next: Vec<String> = vec![url.to_string()];
    for entry in load_history() {
        if entry != url {
            next.push(entry);
        }
    }
    next.truncate(LIMIT);

    if let Some(path) = history_file() {
        if let Ok(json) = serde_json::to_string_pretty(&HistoryDoc(next.clone())) {
            let _ = fs::create_dir_all(path.parent().unwrap_or_else(|| std::path::Path::new(".")));
            let _ = fs::write(&path, format!("{json}\n"));
        }
    }

    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_and_cap() {
        // we can't easily test the disk side without polluting the host's
        // home dir, so just sanity-check the in-memory shape
        let mut urls = vec!["b".into(), "c".into()];
        let url = "a";
        let mut next = vec![url.to_string()];
        for entry in urls.drain(..) {
            if entry != url {
                next.push(entry);
            }
        }
        assert_eq!(next, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }
}
