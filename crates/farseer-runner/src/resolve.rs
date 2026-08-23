//! Explicit `PATHEXT` resolution before a bare command name is trusted.
//!
//! `AGENTS.md`: an extension-less `npm` on `PATH` is a POSIX shell script, not
//! `npm.cmd`. `jobspike` proved the trap is real by finding the bare file first
//! when extensions were not tried before it. This is that fix, pulled out of
//! the spike and made pure so the ordering is a test rather than a memory.

use std::path::{Path, PathBuf};

/// Pure core: given the candidate directories, `PATHEXT` and a file-existence
/// check, find the first match. `PATHEXT` candidates are tried **before** the
/// bare name in every directory, which is the ordering the spike's regression
/// depends on.
pub fn resolve_with(
    name: &str,
    dirs: impl IntoIterator<Item = PathBuf>,
    pathext: &str,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let exts: Vec<String> = pathext
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();

    for dir in dirs {
        for ext in &exts {
            let cand = dir.join(format!("{name}{ext}"));
            if is_file(&cand) {
                return Some(cand);
            }
        }
        let bare = dir.join(name);
        let bare_has_exec_ext = bare
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| exts.contains(&format!(".{}", e.to_ascii_lowercase())))
            .unwrap_or(false);
        if bare_has_exec_ext && is_file(&bare) {
            return Some(bare);
        }
    }
    None
}

/// The real thing: `PATH` and `PATHEXT` from the environment, the real
/// filesystem.
pub fn resolve(name: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    resolve_with(name, dirs, &pathext, |p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn pathext_candidates_are_tried_before_the_bare_name() {
        // The exact trap `jobspike` found: `npm` (a shell script with no
        // extension) sits next to `npm.cmd` in the same directory.
        let dir = PathBuf::from(r"C:\node");
        let existing: HashSet<PathBuf> =
            [dir.join("npm"), dir.join("npm.cmd")].into_iter().collect();

        let found = resolve_with("npm", [dir.clone()], ".COM;.EXE;.BAT;.CMD", |p| {
            existing.contains(p)
        });

        assert_eq!(found, Some(dir.join("npm.cmd")));
    }

    #[test]
    fn later_directories_are_only_reached_if_earlier_ones_have_nothing() {
        let first = PathBuf::from(r"C:\first");
        let second = PathBuf::from(r"C:\second");
        let existing: HashSet<PathBuf> = [second.join("claude.exe")].into_iter().collect();

        let found = resolve_with("claude", [first, second.clone()], ".EXE", |p| {
            existing.contains(p)
        });

        assert_eq!(found, Some(second.join("claude.exe")));
    }

    #[test]
    fn nothing_found_anywhere_is_none() {
        let found = resolve_with("claude", [PathBuf::from(r"C:\empty")], ".EXE", |_| false);
        assert_eq!(found, None);
    }
}
