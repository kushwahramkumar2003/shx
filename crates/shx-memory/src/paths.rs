//! OS default DB location (docs/03-MEMORY.md §2).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::store::{MemoryError, Result};

/// `~/.local/share/shx/shx.db` (Linux), Application Support (macOS),
/// `%LOCALAPPDATA%\shx\shx.db` (Windows). `[memory] path` overrides at the
/// caller.
pub fn default_db_path() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("shx").join("shx.db");
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("shx")
                .join("shx.db");
        }
    }
    #[cfg(windows)]
    {
        if let Some(local) = env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("shx").join("shx.db");
        }
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("shx")
            .join("shx.db");
    }
    PathBuf::from("shx.db")
}

/// Create the parent directory with `0700` on Unix.
pub fn ensure_parent(path: &Path) -> Result<()> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(dir)
        .map_err(|e| MemoryError::Io(format!("create {}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| MemoryError::Io(format!("chmod 0700 {}: {e}", dir.display())))?;
    }
    Ok(())
}

/// Set `0600` on the DB file (Unix).
pub fn restrict_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| MemoryError::Io(format!("chmod 0600 {}: {e}", path.display())))?;
    }
    let _ = path;
    Ok(())
}
