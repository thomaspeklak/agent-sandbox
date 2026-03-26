use std::collections::BTreeSet;
use std::fs;

use crate::util::is_executable;

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

/// A preset for a commonly-used CLI tool binary.
pub struct ToolPreset {
    pub binary_names: &'static [&'static str],
}

/// A preset for a commonly-used host path.
pub struct MountPreset {
    pub host: &'static str,
}

/// Built-in tool presets for popular CLI utilities.
pub const TOOL_PRESETS: &[ToolPreset] = &[
    ToolPreset {
        binary_names: &["gh"],
    },
    ToolPreset {
        binary_names: &["jq"],
    },
    ToolPreset {
        binary_names: &["rg"],
    },
    ToolPreset {
        binary_names: &["fd", "fdfind"],
    },
    ToolPreset {
        binary_names: &["fzf"],
    },
    ToolPreset {
        binary_names: &["delta"],
    },
    ToolPreset {
        binary_names: &["bat", "batcat"],
    },
    ToolPreset {
        binary_names: &["eza", "exa"],
    },
    ToolPreset {
        binary_names: &["zoxide"],
    },
    ToolPreset {
        binary_names: &["starship"],
    },
];

/// Built-in mount presets for common config/auth paths.
pub const MOUNT_PRESETS: &[MountPreset] = &[
    MountPreset { host: "~/.docker" },
    MountPreset {
        host: "~/.config/mcp",
    },
    MountPreset {
        host: "~/.ssh/known_hosts",
    },
    MountPreset {
        host: "~/.gitconfig",
    },
    MountPreset {
        host: "~/.config/gh",
    },
    MountPreset { host: "~/.aws" },
    MountPreset { host: "~/.npmrc" },
    MountPreset { host: "~/.cargo" },
];

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Discover executables on `$PATH`.
///
/// Scans every directory listed in the `PATH` environment variable, collects
/// file names that are executable (via `crate::util::is_executable`), filters
/// obvious noise (dot-files, names containing only punctuation), and returns
/// the result sorted alphabetically with duplicates removed.
pub fn discover_path_binaries() -> Vec<String> {
    let path_var = match std::env::var_os("PATH") {
        Some(v) => v,
        None => return Vec::new(),
    };

    let mut names = BTreeSet::new();

    for dir in std::env::split_paths(&path_var) {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();

            // Skip dot-files
            if name.starts_with('.') {
                continue;
            }

            // Skip names that are only punctuation / whitespace
            if !name.chars().any(|c| c.is_alphanumeric()) {
                continue;
            }

            if let Ok(ft) = entry.file_type()
                && ft.is_file()
            {
                let path = entry.path();
                if is_executable(&path) {
                    names.insert(name.into_owned());
                }
            }
        }
    }

    names.into_iter().collect()
}

/// Discover common config/state directories under `$HOME`.
///
/// Checks for existing directories under `~/.config/*`, `~/.local/share/*`,
/// `~/.cache/*`, and top-level dot-directories/dot-files in `$HOME`.
/// Only paths that actually exist on the host are returned, as UTF-8 strings.
pub fn discover_home_dirs() -> Vec<String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    let sub_roots = [
        home.join(".config"),
        home.join(".local").join("share"),
        home.join(".cache"),
    ];

    for root in &sub_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type()
                    && ft.is_dir()
                    && let Some(s) = entry.path().to_str()
                {
                    results.push(s.to_string());
                }
            }
        }
    }

    // Top-level dot-dirs/dot-files
    if let Ok(entries) = fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                && name.len() > 1
                && name != ".."
                && let Ok(ft) = entry.file_type()
                && (ft.is_dir() || ft.is_file())
                && let Some(s) = entry.path().to_str()
            {
                results.push(s.to_string());
            }
        }
    }

    results.sort();
    results.dedup();
    results
}

// ---------------------------------------------------------------------------
// Fuzzy matching
// ---------------------------------------------------------------------------

/// Score a candidate against a query. Higher = better match. 0 = no match.
///
/// Scoring tiers:
/// - **Exact match** (case-insensitive): 1000
/// - **Prefix match**: 750 + (length_ratio * 200)
/// - **Substring match**: 500 + (length_ratio * 200)
/// - **Fuzzy match** (all query chars appear in order): 100 + (consecutive_bonus * 10)
pub fn fuzzy_score(query: &str, candidate: &str) -> u32 {
    if query.is_empty() {
        return 1; // empty query matches everything with minimal score
    }
    if candidate.is_empty() {
        return 0;
    }

    let q = query.to_ascii_lowercase();
    let c = candidate.to_ascii_lowercase();

    // Exact match
    if q == c {
        return 1000;
    }

    // Prefix match
    if c.starts_with(&q) {
        let ratio = (q.len() as f64) / (c.len() as f64);
        return 750 + (ratio * 200.0) as u32;
    }

    // Substring match
    if c.contains(&q) {
        let ratio = (q.len() as f64) / (c.len() as f64);
        return 500 + (ratio * 200.0) as u32;
    }

    // Fuzzy: every query char must appear in order in the candidate
    let mut q_iter = q.chars().peekable();
    let mut consecutive = 0u32;
    let mut max_consecutive = 0u32;
    let mut prev_matched = false;

    for ch in c.chars() {
        if q_iter.peek() == Some(&ch) {
            q_iter.next();
            if prev_matched {
                consecutive += 1;
                max_consecutive = max_consecutive.max(consecutive);
            } else {
                consecutive = 1;
            }
            prev_matched = true;
        } else {
            prev_matched = false;
        }
    }

    if q_iter.peek().is_some() {
        // Not all query chars were found
        return 0;
    }

    100 + max_consecutive * 10
}

/// Filter and rank candidates by fuzzy match score.
///
/// Returns only candidates with a non-zero score, sorted from highest to
/// lowest score.
pub fn fuzzy_filter<'a>(query: &str, candidates: &'a [String]) -> Vec<(&'a str, u32)> {
    let mut scored: Vec<(&str, u32)> = candidates
        .iter()
        .filter_map(|c| {
            let s = fuzzy_score(query, c);
            if s > 0 { Some((c.as_str(), s)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    scored
}

// ---------------------------------------------------------------------------
// Unified suggestion API
// ---------------------------------------------------------------------------

/// A ranked suggestion returned by the suggestion helpers.
pub struct Suggestion {
    pub value: String,
    pub score: u32,
}

/// Merge preset suggestions with discovered candidates, deduplicate, and sort.
fn merge_suggestions(
    presets: Vec<Suggestion>,
    discovered: &[String],
    query: &str,
) -> Vec<Suggestion> {
    let mut suggestions = presets;

    let filtered = fuzzy_filter(query, discovered);
    for (name, score) in filtered {
        // Linear scan is fine — presets list has at most ~12 entries.
        if !suggestions.iter().any(|s| s.value == name) {
            suggestions.push(Suggestion {
                value: name.to_string(),
                score,
            });
        }
    }

    suggestions.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.value.cmp(&b.value)));
    suggestions
}

/// Get suggestions for a binary name input using pre-cached PATH binaries.
///
/// Combines preset tools and the provided cached binaries, scores them against
/// the query, and returns ranked suggestions.
pub fn suggest_binaries_from(query: &str, cached_binaries: &[String]) -> Vec<Suggestion> {
    let mut presets = Vec::new();
    for preset in TOOL_PRESETS {
        for &bin in preset.binary_names {
            let score = fuzzy_score(query, bin);
            if score > 0 {
                presets.push(Suggestion {
                    value: bin.to_string(),
                    score,
                });
            }
        }
    }

    merge_suggestions(presets, cached_binaries, query)
}

/// Get suggestions for a host path input using pre-cached home directories.
///
/// Combines preset mount paths and the provided cached directories, scores them
/// against the query, and returns ranked suggestions.
pub fn suggest_paths_from(query: &str, cached_dirs: &[String]) -> Vec<Suggestion> {
    let mut presets = Vec::new();
    for preset in MOUNT_PRESETS {
        let score = fuzzy_score(query, preset.host);
        if score > 0 {
            presets.push(Suggestion {
                value: preset.host.to_string(),
                score,
            });
        }
    }

    merge_suggestions(presets, cached_dirs, query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_scores_highest() {
        assert_eq!(fuzzy_score("jq", "jq"), 1000);
    }

    #[test]
    fn prefix_beats_substring() {
        let prefix = fuzzy_score("ba", "bat");
        let substr = fuzzy_score("ba", "foobar");
        assert!(prefix > substr);
    }

    #[test]
    fn substring_beats_fuzzy() {
        let substr = fuzzy_score("grep", "ripgrep");
        let fuzzy = fuzzy_score("grep", "g_r_e_p_tool");
        assert!(substr > fuzzy);
    }

    #[test]
    fn no_match_returns_zero() {
        assert_eq!(fuzzy_score("xyz", "abc"), 0);
    }

    #[test]
    fn empty_query_matches_all() {
        assert!(fuzzy_score("", "anything") > 0);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(fuzzy_score("JQ", "jq"), 1000);
        assert_eq!(fuzzy_score("jq", "JQ"), 1000);
    }

    #[test]
    fn fuzzy_filter_ranks_correctly() {
        let candidates = vec![
            "bat".to_string(),
            "cat".to_string(),
            "batcat".to_string(),
            "foobar".to_string(),
        ];
        let results = fuzzy_filter("bat", &candidates);
        assert_eq!(results[0].0, "bat"); // exact
        assert_eq!(results[1].0, "batcat"); // prefix
    }
}
