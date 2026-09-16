//! Pure environment and project scoping helpers for `shx-core`.
//!
//! No I/O. Computes deterministic `project_id` hashes and performs
//! pure container detection against injected filesystem probes.

/// 64-bit FNV-1a hash offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
/// 64-bit FNV-1a prime.
const FNV_PRIME: u64 = 0x100000001b3;

/// Compute a 64-bit FNV-1a hash of bytes.
pub fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Normalize a git root path for stable hash generation.
///
/// Converts Windows backslashes to forward slashes and trims trailing slashes
/// (unless the path is exactly `/`).
pub fn normalize_git_root(path: &str) -> String {
    let trimmed = path.trim();
    let forward_slashed = trimmed.replace('\\', "/");
    let without_trailing = forward_slashed.trim_end_matches('/');
    if without_trailing.is_empty() && forward_slashed.starts_with('/') {
        "/".to_string()
    } else {
        without_trailing.to_string()
    }
}

/// Compute a deterministic `project_id` hex string from a normalized git root path.
///
/// Two distinct repository roots will produce distinct IDs; identical roots
/// (differing only by trailing slashes or backslashes) will produce the same ID.
pub fn compute_project_id(git_root: &str) -> String {
    let normalized = normalize_git_root(git_root);
    let hash = fnv1a_hash(normalized.as_bytes());
    format!("{hash:016x}")
}

/// Pure container detection inspecting file presence, cgroup content, and environment.
///
/// Markers checked:
/// - Presence of `/.dockerenv`
/// - Presence of `/run/.containerenv` (Podman)
/// - Environment variable `container` (podman, lxc, systemd-nspawn)
/// - `/proc/1/cgroup` containing container runtime keywords
pub fn is_in_container<F>(
    file_exists: F,
    cgroup_text: Option<&str>,
    container_env: Option<&str>,
) -> bool
where
    F: Fn(&str) -> bool,
{
    if file_exists("/.dockerenv") || file_exists("/run/.containerenv") {
        return true;
    }
    if let Some(env) = container_env {
        let trimmed = env.trim();
        if !trimmed.is_empty() && trimmed != "0" && trimmed != "false" {
            return true;
        }
    }
    if let Some(cgroup) = cgroup_text {
        let lower = cgroup.to_ascii_lowercase();
        if lower.contains("docker")
            || lower.contains("kubepods")
            || lower.contains("containerd")
            || lower.contains("lxc")
            || lower.contains("libpod")
            || lower.contains("sandbox")
        {
            return true;
        }
    }
    false
}

/// Resolve `in_container` boolean from user config and detected state.
///
/// - `"true"` => forces `true`
/// - `"false"` => forces `false`
/// - `"auto"` | anything else => runs [`is_in_container`]
pub fn resolve_in_container<F>(
    setting: &str,
    file_exists: F,
    cgroup_text: Option<&str>,
    container_env: Option<&str>,
) -> bool
where
    F: Fn(&str) -> bool,
{
    match setting.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => true,
        "false" | "no" | "0" => false,
        _ => is_in_container(file_exists, cgroup_text, container_env),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_id_determinism_and_normalization() {
        let id1 = compute_project_id("/Users/dev/project");
        let id2 = compute_project_id("/Users/dev/project/");
        let id3 = compute_project_id("/Users/dev/other-project");
        let id_win = compute_project_id("C:\\Users\\dev\\project\\");
        let id_win_norm = compute_project_id("C:/Users/dev/project");

        assert_eq!(id1, id2, "trailing slash should normalize identically");
        assert_ne!(
            id1, id3,
            "different project paths must have distinct project_ids"
        );
        assert_eq!(
            id_win, id_win_norm,
            "windows backslashes normalize to forward slashes"
        );
        assert_eq!(id1.len(), 16, "project_id is a 16-character hex string");
    }

    #[test]
    fn container_detection_mocked_fs() {
        // Outside container: no files, clean cgroup, no env
        assert!(!is_in_container(|_| false, None, None));
        assert!(!is_in_container(
            |_| false,
            Some("1:name=systemd:/\n"),
            None
        ));

        // Docker file
        assert!(is_in_container(|p| p == "/.dockerenv", None, None));

        // Podman file
        assert!(is_in_container(|p| p == "/run/.containerenv", None, None));

        // Env var
        assert!(is_in_container(|_| false, None, Some("podman")));
        assert!(is_in_container(|_| false, None, Some("docker")));
        assert!(!is_in_container(|_| false, None, Some("false")));
        assert!(!is_in_container(|_| false, None, Some("0")));
        assert!(!is_in_container(|_| false, None, Some("")));

        // Cgroup content
        assert!(is_in_container(
            |_| false,
            Some("12:pids:/docker/12345abcdef"),
            None
        ));
        assert!(is_in_container(
            |_| false,
            Some("1:name=systemd:/kubepods.slice/kubepods-pod123"),
            None
        ));

        // Config overrides
        assert!(resolve_in_container("true", |_| false, None, None));
        assert!(!resolve_in_container(
            "false",
            |p| p == "/.dockerenv",
            None,
            None
        ));
        assert!(resolve_in_container(
            "auto",
            |p| p == "/.dockerenv",
            None,
            None
        ));
        assert!(!resolve_in_container("auto", |_| false, None, None));
    }
}
