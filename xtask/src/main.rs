//! Dev-task runner for the `shx` workspace.
//!
//! `cargo xtask ci` is the full local gate (same 9 steps CI runs).

#![forbid(unsafe_code)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    match cmd.as_str() {
        "ci" => match ci() {
            Ok(()) => {
                eprintln!("ci: all 9 steps passed");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        },
        "" | "help" | "--help" | "-h" => {
            eprintln!("cargo xtask <ci>");
            ExitCode::from(2)
        }
        other => {
            eprintln!("unknown xtask '{other}'. try: cargo xtask ci");
            ExitCode::from(2)
        }
    }
}

fn ci() -> Result<(), String> {
    let root = workspace_root();
    ensure_ci_tools()?;

    step(1, "cargo fmt --all --check", || {
        cargo(&root, &["fmt", "--all", "--check"])
    })?;
    step(
        2,
        "cargo clippy --all-targets --all-features -- -D warnings",
        || {
            cargo(
                &root,
                &[
                    "clippy",
                    "--all-targets",
                    "--all-features",
                    "--",
                    "-D",
                    "warnings",
                ],
            )
        },
    )?;
    step(3, "cargo test --workspace --all-features", || {
        cargo(&root, &["test", "--workspace", "--all-features"])
    })?;
    step(4, "clippy + test --no-default-features", || {
        cargo(
            &root,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--no-default-features",
                "--",
                "-D",
                "warnings",
            ],
        )?;
        cargo(&root, &["test", "--workspace", "--no-default-features"])
    })?;
    step(5, "cargo deny check", || cargo(&root, &["deny", "check"]))?;
    step(6, "cargo audit", || cargo(&root, &["audit"]))?;
    step(7, "typos", || run(&root, "typos", &[]))?;
    step(8, "cargo doc --no-deps --deny warnings", || {
        let mut cmd = Command::new("cargo");
        cmd.args(["doc", "--workspace", "--no-deps"])
            .env("RUSTDOCFLAGS", "--deny warnings")
            .current_dir(&root);
        status(cmd, "cargo doc --workspace --no-deps")
    })?;
    step(9, "cargo build --release + size check", || {
        cargo(&root, &["build", "--release"])?;
        check_release_size(&root)
    })?;
    Ok(())
}

fn step(n: u8, name: &str, f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    eprintln!("==> {n}/9 {name}");
    f()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is one level below the workspace root")
        .to_path_buf()
}

fn cargo(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.args(args).current_dir(root);
    status(cmd, &format!("cargo {}", args.join(" ")))
}

fn run(root: &Path, bin: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(root);
    status(cmd, bin)
}

fn status(mut cmd: Command, label: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to spawn {label}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed"))
    }
}

fn ensure_ci_tools() -> Result<(), String> {
    ensure_cargo_subcommand("deny", "cargo-deny")?;
    ensure_cargo_subcommand("audit", "cargo-audit")?;
    ensure_bin("typos", "typos-cli")?;
    Ok(())
}

fn ensure_cargo_subcommand(sub: &str, crate_name: &str) -> Result<(), String> {
    if command_ok("cargo", &[sub, "--version"]) {
        return Ok(());
    }
    install_crate(crate_name)
}

fn ensure_bin(bin: &str, crate_name: &str) -> Result<(), String> {
    if command_ok(bin, &["--version"]) {
        return Ok(());
    }
    install_crate(crate_name)
}

fn command_ok(bin: &str, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn install_crate(crate_name: &str) -> Result<(), String> {
    eprintln!("installing {crate_name} (required by cargo xtask ci)…");
    let status = Command::new("cargo")
        .args(["install", crate_name, "--locked"])
        .status()
        .map_err(|e| format!("failed to install {crate_name}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo install {crate_name} --locked failed; install it manually and re-run"
        ))
    }
}

fn check_release_size(root: &Path) -> Result<(), String> {
    let mut bin = root.join("target/release");
    if cfg!(windows) {
        bin.push("shx.exe");
    } else {
        bin.push("shx");
    }
    let bytes = std::fs::metadata(&bin)
        .map_err(|e| format!("release binary {} missing: {e}", bin.display()))?
        .len();
    let mb = bytes as f64 / (1024.0 * 1024.0);
    eprintln!(
        "release binary {} is {mb:.2} MB ({bytes} bytes)",
        bin.display()
    );
    if mb > 25.0 {
        return Err(format!(
            "binary size {mb:.2} MB exceeds the 25 MB hard limit"
        ));
    }
    if mb > 15.0 {
        eprintln!("warning: binary exceeds the 15 MB warn threshold");
    }
    Ok(())
}
