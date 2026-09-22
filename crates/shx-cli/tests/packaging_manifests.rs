//! T-607: scoop/winget manifest templates are well-formed and tokenized for
//! the release fill job. Portable string/JSON assertions only (no YAML crate).

use std::fs;
use std::path::PathBuf;

fn packaging() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging")
        .canonicalize()
        .expect("packaging dir")
}

/// Scoop bucket manifest parses and carries versioned install + autoupdate.
#[test]
fn t_607_scoop_template_shape() {
    let text = fs::read_to_string(packaging().join("scoop/shx.json")).expect("scoop manifest");
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(v["version"], "__SHX_VERSION__");
    assert!(v["description"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(
        v["homepage"]
            .as_str()
            .is_some_and(|s| s.contains("github.com"))
    );
    let arch = &v["architecture"]["64bit"];
    assert_eq!(arch["bin"], "shx.exe");
    assert_eq!(arch["extract_dir"], "shx-x86_64-pc-windows-msvc");
    assert_eq!(arch["hash"], "__SHX_WINDOWS_X64_SHA256__");
    assert!(
        arch["url"].as_str().is_some_and(
            |u| u.contains("__SHX_VERSION__") && u.ends_with("shx-x86_64-pc-windows-msvc.zip")
        ),
        "versioned asset URL: {}",
        arch["url"]
    );
    // Autoupdate keeps future hashes fresh; $version is scoop's own variable.
    assert!(v["autoupdate"].is_object());
    assert!(v["checkver"]["regex"].is_string());
    assert!(text.contains("$version"), "scoop update variable intact");
}

/// Winget triple: version/locale/installer with tokens and a static x64 entry.
#[test]
fn t_607_winget_template_shape() {
    let dir = packaging().join("winget");
    let version = fs::read_to_string(dir.join("shx.version.yaml")).expect("version manifest");
    let locale = fs::read_to_string(dir.join("shx.locale.en-US.yaml")).expect("locale manifest");
    let installer = fs::read_to_string(dir.join("shx.installer.yaml")).expect("installer manifest");
    for (name, text) in [
        ("version", &version),
        ("locale", &locale),
        ("installer", &installer),
    ] {
        assert!(
            text.contains("PackageIdentifier: shx.shx"),
            "{name}: identifier"
        );
        assert!(
            text.contains("PackageVersion: __SHX_VERSION__"),
            "{name}: version token"
        );
        assert!(text.contains("ManifestVersion: 1.4.0"), "{name}: schema");
    }
    assert!(locale.contains("Publisher:"), "locale names a publisher");
    assert!(
        installer.contains("InstallerType: zip"),
        "zip over the dist archive"
    );
    assert!(
        installer.contains("shx-x86_64-pc-windows-msvc/shx.exe"),
        "static nested exe path matching the archive layout"
    );
    assert!(
        installer.contains("Architecture: x64"),
        "only targets we build"
    );
    assert!(
        installer.contains("InstallerSha256: __SHX_WINDOWS_X64_SHA256__"),
        "hash token for the fill job"
    );
}

/// Placeholders and tokens are documented, not silent.
#[test]
fn t_607_placeholders_documented() {
    let readme = fs::read_to_string(packaging().join("README.md")).expect("packaging README");
    assert!(readme.contains("<org>"), "placeholder convention");
    assert!(readme.contains("__SHX_WINDOWS_X64_SHA256__"), "token docs");
    assert!(readme.contains("winget-pkgs"), "submission path");
}
