# packaging

Windows package-manager manifests for `shx` releases. cargo-dist 0.32 has no
scoop or winget support, so these are versioned **templates**: the committed
files carry `__SHX_VERSION__` and `__SHX_WINDOWS_X64_SHA256__` tokens, and
`.github/workflows/packaging-manifests.yml` fills them from the published
release (tag version + real asset hash) and uploads the filled copies back
to the GitHub Release on every `release: published` event.

## Placeholders to replace when the org exists

- `https://github.com/<org>/shx` — the repository (same `<org>` convention
  as `CHANGELOG.md` and `Cargo.toml`).
- `PackageIdentifier: shx.shx` / `Publisher: shx` (winget) — minimal
  placeholder identity; rename both together when the publisher is known.
- Scoop `checkver`/`autoupdate` stanzas refresh hashes automatically after
  the first filled release.

## Files

- `scoop/shx.json` — Scoop bucket manifest (x64 Windows zip, `extract_dir`
  matches the cargo-dist archive layout).
- `winget/shx.{version,locale.en-US,installer}.yaml` — winget-pkgs
  submission triple (ManifestVersion 1.4.0). The installer entry is x64
  only: no ARM64 Windows target is built (see `dist-workspace.toml`).

## winget-pkgs submission (manual, once per release)

The filled manifests attached to the GitHub Release still need a PR to
[winget-pkgs](https://github.com/microsoft/winget-pkgs) (e.g. via
`wingetcreate update shx.shx -v <version> --submit` against the release
URLs), which requires a publisher identity — out of scope until then.
