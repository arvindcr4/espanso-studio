//! Core espanso-config file store. Pure Rust, no Tauri imports, so it can be
//! unit-tested on any platform with `cargo test`.
//!
//! Path resolution mirrors espanso's own (espanso/src/path/mod.rs):
//! dirs::config_dir()/espanso, plus the legacy macOS ~/Library/Preferences/espanso
//! fallback. Snippet sets are the YAML files under <root>/match/.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct SetInfo {
    /// Path relative to the match dir, e.g. "base.yml" or "work/jira.yml"
    pub name: String,
    /// Absolute path on disk
    pub path: String,
    /// Raw file contents
    pub yaml: String,
    /// YAML parse error, if the file is currently invalid
    pub error: Option<String>,
}

pub fn config_root() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        // espanso originally used ~/Library/Preferences/espanso; if a user
        // still has that directory it is the live one (espanso issue #611).
        if let Some(pref) = dirs::preference_dir() {
            let legacy = pref.join("espanso");
            if legacy.is_dir() {
                return Ok(legacy);
            }
        }
    }
    let base = dirs::config_dir().ok_or("could not resolve the OS config directory")?;
    Ok(base.join("espanso"))
}

pub fn match_dir(root: &Path) -> PathBuf {
    root.join("match")
}

pub fn config_dir_display() -> Result<String, String> {
    Ok(config_root()?.display().to_string())
}

pub fn list_sets() -> Result<Vec<SetInfo>, String> {
    let root = config_root()?;
    list_sets_in(&match_dir(&root))
}

pub fn save_set(path: &str, yaml: &str) -> Result<(), String> {
    let root = config_root()?;
    save_set_in(&match_dir(&root), Path::new(path), yaml)
}

pub fn create_set(name: &str) -> Result<SetInfo, String> {
    let root = config_root()?;
    create_set_in(&match_dir(&root), name)
}

// ---------- testable core ----------

fn list_sets_in(mdir: &Path) -> Result<Vec<SetInfo>, String> {
    let mut out = Vec::new();
    if mdir.is_dir() {
        let mut stack = vec![mdir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries =
                fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
            for entry in entries.flatten() {
                let p = entry.path();
                let hidden = p
                    .file_name()
                    .map(|n| {
                        let s = n.to_string_lossy();
                        s.starts_with('_') || s.starts_with('.')
                    })
                    .unwrap_or(true);
                if hidden {
                    continue; // espanso's convention: _private dirs, .dotfiles are not sets
                }
                if p.is_dir() {
                    stack.push(p);
                } else if is_yaml(&p) {
                    let rel = p.strip_prefix(mdir).unwrap_or(&p).display().to_string();
                    let yaml = fs::read_to_string(&p).unwrap_or_default();
                    let error = match serde_yaml::from_str::<serde_yaml::Value>(&yaml) {
                        Ok(_) => None,
                        Err(e) => Some(e.to_string()),
                    };
                    out.push(SetInfo { name: rel, path: p.display().to_string(), yaml, error });
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn save_set_in(mdir: &Path, target: &Path, yaml: &str) -> Result<(), String> {
    // never write invalid YAML into the live config
    serde_yaml::from_str::<serde_yaml::Value>(yaml).map_err(|e| format!("invalid YAML: {e}"))?;
    guard_inside_match_dir(mdir, target)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if target.exists() {
        // one cheap undo level: base.yml.bak
        let bak = target.with_extension("yml.bak");
        fs::copy(target, &bak).map_err(|e| format!("backup failed: {e}"))?;
    }
    // atomic-ish write: temp file then rename, so espanso's file watcher
    // never sees a half-written file
    let tmp = target.with_extension("studio-tmp");
    fs::write(&tmp, yaml).map_err(|e| e.to_string())?;
    fs::rename(&tmp, target).map_err(|e| e.to_string())?;
    Ok(())
}

fn create_set_in(mdir: &Path, name: &str) -> Result<SetInfo, String> {
    let clean: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let clean = clean.trim_matches('-');
    if clean.is_empty() {
        return Err("set name needs at least one letter or number".into());
    }
    fs::create_dir_all(mdir).map_err(|e| e.to_string())?;
    let path = mdir.join(format!("{clean}.yml"));
    if path.exists() {
        return Err(format!("{clean}.yml already exists"));
    }
    fs::write(&path, "# Espanso Studio snippet set\nmatches: []\n").map_err(|e| e.to_string())?;
    Ok(SetInfo {
        name: format!("{clean}.yml"),
        path: path.display().to_string(),
        yaml: "matches: []".into(),
        error: None,
    })
}

fn is_yaml(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()), Some("yml") | Some("yaml"))
}

fn guard_inside_match_dir(mdir: &Path, target: &Path) -> Result<(), String> {
    let m = mdir.canonicalize().unwrap_or_else(|_| mdir.to_path_buf());
    let t = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    if t.starts_with(&m) {
        Ok(())
    } else {
        Err("refusing to write outside the espanso match directory".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("espanso-studio-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_create_save_list() {
        let mdir = temp_root("rt");
        let created = create_set_in(&mdir, "My Work Set").unwrap();
        assert_eq!(created.name, "My-Work-Set.yml");
        let yaml = "matches:\n  - trigger: \":sig\"\n    replace: \"hello\"\n";
        save_set_in(&mdir, Path::new(&created.path), yaml).unwrap();
        let sets = list_sets_in(&mdir).unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].yaml, yaml);
        assert!(sets[0].error.is_none());
        assert!(mdir.join("My-Work-Set.yml.bak").exists(), "backup written");
    }

    #[test]
    fn rejects_invalid_yaml() {
        let mdir = temp_root("bad");
        let created = create_set_in(&mdir, "ok").unwrap();
        let res = save_set_in(&mdir, Path::new(&created.path), "matches: [unclosed");
        assert!(res.is_err());
    }

    #[test]
    fn refuses_paths_outside_match_dir() {
        let mdir = temp_root("guard");
        let outside = std::env::temp_dir().join("evil.yml");
        let res = save_set_in(&mdir, &outside, "matches: []");
        assert!(res.is_err());
    }
}
