//! Project scoping and container runtime detection for `shx-memory`.
//!
//! Provides git root discovery, `project_id` resolution (keyed by git root),
//! and filesystem-backed container detection.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use shx_core::compute_project_id;
use shx_core::env::resolve_in_container;
use shx_core::{Interaction, Scope};

use crate::store::{MemoryStore, Result};

/// Walk up from `start_dir` to find the enclosing git repository root, if any.
///
/// Looks for a `.git` directory or file (worktree/submodule) in `start_dir`
/// and its ancestors. Returns the canonical path if successful.
pub fn find_git_root(start_dir: &Path) -> Option<PathBuf> {
    for ancestor in start_dir.ancestors() {
        if ancestor.join(".git").exists() {
            return Some(fs::canonicalize(ancestor).unwrap_or_else(|_| ancestor.to_path_buf()));
        }
    }
    None
}

/// Resolve the project root path and its deterministic `project_id`.
///
/// If `override_path` is specified (`--project <path>`), resolves that path:
/// if it contains or is within a git repository, that repository root is used;
/// otherwise, the directory itself serves as the project root.
///
/// If `override_path` is `None`, attempts to find the git repository enclosing `cwd`.
/// Outside any repository, returns `(None, None)`.
pub fn resolve_project_scope(
    override_path: Option<&str>,
    cwd: &Path,
) -> (Option<PathBuf>, Option<String>) {
    if let Some(raw) = override_path {
        let p = Path::new(raw);
        let path = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        if let Some(root) = find_git_root(&path) {
            let id = compute_project_id(&root.to_string_lossy());
            return (Some(root), Some(id));
        }
        if path.exists() {
            let canon = fs::canonicalize(&path).unwrap_or(path);
            let id = compute_project_id(&canon.to_string_lossy());
            return (Some(canon), Some(id));
        }
        return (None, None);
    }

    if let Some(root) = find_git_root(cwd) {
        let id = compute_project_id(&root.to_string_lossy());
        (Some(root), Some(id))
    } else {
        (None, None)
    }
}

/// Detect whether the current process is executing within a container environment.
///
/// Reads filesystem markers (`/.dockerenv`, `/run/.containerenv`), `/proc/1/cgroup`
/// on Linux, and the `container` environment variable, respecting the `setting`
/// knob (`"auto"`, `"true"`, or `"false"`).
pub fn detect_in_container(setting: &str) -> bool {
    let cgroup = fs::read_to_string("/proc/1/cgroup").ok();
    let container_env = env::var("container").ok();

    resolve_in_container(
        setting,
        |p| Path::new(p).exists(),
        cgroup.as_deref(),
        container_env.as_deref(),
    )
}

/// Fetch recent interactions strictly scoped to `project_id`.
pub fn recall_project_history(
    store: &dyn MemoryStore,
    project_id: &str,
    limit: usize,
) -> Result<Vec<Interaction>> {
    store.recent(
        limit,
        Scope::Project {
            id: Some(project_id.to_string()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = env::temp_dir().join(format!("shx-scope-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn git_root_discovery_and_ancestors() {
        let root = temp_dir("repo");
        fs::create_dir_all(root.join(".git")).expect("create .git");
        let sub = root.join("crates").join("nested");
        fs::create_dir_all(&sub).expect("create subdirs");

        let found = find_git_root(&sub);
        assert!(found.is_some(), "should find git root from subdirs");
        let canon_root = fs::canonicalize(&root).unwrap();
        assert_eq!(found.unwrap(), canon_root);

        let non_repo = temp_dir("non-repo");
        assert!(find_git_root(&non_repo).is_none());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(non_repo);
    }

    #[test]
    fn resolve_scope_with_and_without_override() {
        let repo_a = temp_dir("repo-a");
        fs::create_dir_all(repo_a.join(".git")).expect("create .git");
        let repo_b = temp_dir("repo-b");
        fs::create_dir_all(repo_b.join(".git")).expect("create .git");

        // From within repo_a without override
        let (root_a, id_a) = resolve_project_scope(None, &repo_a);
        assert!(root_a.is_some());
        assert!(id_a.is_some());

        // From within repo_a with override pointing to repo_b
        let (root_b, id_b) = resolve_project_scope(Some(repo_b.to_str().unwrap()), &repo_a);
        assert!(root_b.is_some());
        assert!(id_b.is_some());

        assert_ne!(
            id_a.unwrap(),
            id_b.unwrap(),
            "two different repos must produce distinct project_ids"
        );

        let _ = fs::remove_dir_all(repo_a);
        let _ = fs::remove_dir_all(repo_b);
    }
}
