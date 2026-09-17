//! `shx completion <shell>` and `shx man` (T-604).
//!
//! Both artifacts are generated from the clap definition, so they can never
//! drift from the real flags: `man/shx.1` and `contrib/completions/*` are
//! committed snapshots of exactly what these commands print (drift-tested),
//! ready for release archives to ship (T-607) so users don't need the binary
//! to install them. Pure generation: no network, no subprocess, no execution.

use clap::CommandFactory;
use clap_complete::Shell;

use crate::render::{EXIT_ERROR, EXIT_OK, EXIT_USAGE};
use crate::{Cli, Commands};

/// Supported shells (lowercase input; `Shell` itself is case-sensitive).
const SHELLS: &[&str] = &["bash", "elvish", "fish", "powershell", "zsh"];

/// Parse a shell name (case-insensitive) into a clap_complete generator.
fn parse_shell(raw: &str) -> Result<Shell, String> {
    let lowered = raw.trim().to_ascii_lowercase();
    lowered
        .parse::<Shell>()
        .map_err(|_| format!("invalid shell {raw:?}; accepted: {}", SHELLS.join(", ")))
}

/// Completion script for `shell`, generated from the clap definition.
fn generate_completion(shell: Shell) -> String {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "shx", &mut buf);
    String::from_utf8(buf).unwrap_or_default()
}

/// Roff man page generated from the clap definition.
fn generate_man() -> Result<String, String> {
    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buf = Vec::new();
    man.render(&mut buf)
        .map_err(|e| format!("render man page: {e}"))?;
    String::from_utf8(buf).map_err(|e| format!("man page is not UTF-8: {e}"))
}

/// Dispatch `shx completion` / `shx man`.
pub fn run(cli: &Cli) -> i32 {
    match &cli.command {
        Some(Commands::Completion { shell }) => {
            let raw = match shell.as_deref() {
                Some(s) => s,
                None => {
                    eprintln!("shx completion: usage: shx completion <shell>");
                    eprintln!("accepted shells: {}", SHELLS.join(", "));
                    return EXIT_USAGE;
                }
            };
            let parsed = match parse_shell(raw) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("shx completion: {msg}");
                    return EXIT_USAGE;
                }
            };
            print!("{}", generate_completion(parsed));
            EXIT_OK
        }
        Some(Commands::Man) => match generate_man() {
            Ok(page) => {
                print!("{page}");
                EXIT_OK
            }
            Err(msg) => {
                eprintln!("shx: {msg}");
                EXIT_ERROR
            }
        },
        _ => EXIT_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_parse_case_insensitively() {
        for name in [
            "bash",
            "Bash",
            "BASH",
            "zsh",
            "ZSH",
            "fish",
            "powershell",
            "elvish",
            "zsh ",
        ] {
            assert!(parse_shell(name).is_ok(), "{name}");
        }
        for bad in ["", "  ", "sh", "cmd", "bash-completion", "nushell"] {
            assert!(parse_shell(bad).is_err(), "{bad:?}");
        }
        let err = parse_shell("nushell").expect_err("unknown shell");
        assert!(err.contains("bash") && err.contains("zsh"), "{err}");
    }

    #[test]
    fn completion_scripts_carry_shell_markers() {
        let cases = [
            (Shell::Bash, "complete"),
            (Shell::Zsh, "#compdef"),
            (Shell::Fish, "complete"),
            (Shell::PowerShell, "Register-ArgumentCompleter"),
            (Shell::Elvish, "edit:completion:arg-completer"),
        ];
        for (shell, marker) in cases {
            let script = generate_completion(shell);
            assert!(!script.trim().is_empty(), "{shell:?}");
            assert!(
                script.contains(marker),
                "{shell:?} script missing {marker:?}"
            );
            assert!(
                script.contains("shx"),
                "{shell:?} script never mentions shx"
            );
        }
    }

    #[test]
    fn man_page_is_roff_for_shx() {
        let page = generate_man().expect("man renders");
        assert!(page.contains(".TH"), "roff title header");
        assert!(page.contains("shx"), "names the binary");
        assert!(page.contains("completion"), "documents subcommands");
    }
}
