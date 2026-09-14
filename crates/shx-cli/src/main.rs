//! `shx` CLI — print-only natural-language to shell-command translator.
//!
//! Scaffold only. The clap tree and pipeline land in T-006.

#![forbid(unsafe_code)]

fn main() {
    let mut args = std::env::args();
    let _argv0 = args.next();

    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("shx {}", env!("CARGO_PKG_VERSION"));
        }
        Some("--help" | "-h") => {
            print_help();
        }
        _ => {
            // Human-facing text on stderr. stdout stays the command channel.
            eprintln!("shx: command translation is not implemented yet");
            std::process::exit(2);
        }
    }
}

fn print_help() {
    eprintln!(
        "\
shx {}
Natural language in. Shell command out. Nothing executed.

USAGE:
    shx [OPTIONS] <INTENT>...
    shx <SUBCOMMAND> [ARGS]

OPTIONS:
    -h, --help       Print help
    -V, --version    Print version",
        env!("CARGO_PKG_VERSION")
    );
}
