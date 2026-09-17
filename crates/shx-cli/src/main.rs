//! `shx` CLI — print-only natural-language to shell-command translator.
//!
//! stdout is the command channel (ADR-001). This binary never executes a
//! command (ADR-002).

#![forbid(unsafe_code)]

mod commands;
mod pipeline;
mod render;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use shx_config::load;

use pipeline::{PipelineError, flag_overrides};
use render::{EXIT_BACKEND, EXIT_ERROR, EXIT_USAGE};

/// Natural language in. Shell command out. Nothing executed.
#[derive(Debug, Clone, Parser)]
#[command(name = "shx", version, about)]
#[command(after_help = "stdout is the command only; explanations and warnings go to stderr.")]
pub struct Cli {
    /// Return up to N candidates (one command per stdout line).
    #[arg(short = 'n', long = "count", value_name = "N")]
    pub count: Option<u8>,

    /// Structured JSON object on stdout.
    #[arg(long)]
    pub json: bool,

    /// Reverse mode: explain an existing command (T-601).
    #[arg(long, value_name = "CMD")]
    pub explain: Option<String>,

    /// Refine session (T-602).
    #[arg(short = 'i', long)]
    pub interactive: bool,

    /// Show which memory entries / routing were used (stderr).
    #[arg(long)]
    pub why: bool,

    /// Force the local backend.
    #[arg(long, conflicts_with = "cloud")]
    pub local: bool,

    /// Force the cloud backend (error if unconfigured).
    #[arg(long)]
    pub cloud: bool,

    /// Forbid network; use mock/fixtures.
    #[arg(long)]
    pub offline: bool,

    /// Copy the result to the clipboard (T-603).
    #[arg(long)]
    pub copy: bool,

    /// Exit 3 if risk ≥ Review.
    #[arg(long)]
    pub exit_on_risk: bool,

    /// Ignore memory for this call.
    #[arg(long)]
    pub no_memory: bool,

    /// Disable ANSI on stderr.
    #[arg(long)]
    pub no_color: bool,

    /// Named context profile.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Override project scoping.
    #[arg(long, value_name = "PATH")]
    pub project: Option<String>,

    /// Skip tool-side prompts (never execution).
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Raw model output and full error chains on stderr.
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Suppress non-essential stderr.
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Alternate config file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Natural-language intent (joined with spaces).
    #[arg(trailing_var_arg = true)]
    pub intent: Vec<String>,
}

#[derive(Debug, Clone, Subcommand)]
enum Commands {
    /// Self-check config, DB, and backends (T-007).
    Doctor {
        /// JSON report.
        #[arg(long)]
        json: bool,
        /// Run the redaction corpus and report per-pattern pass/fail.
        #[arg(long)]
        redaction_test: bool,
    },
    /// Browse recorded translations (T-204).
    History {
        /// Max rows (list/export).
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Restrict to the current project_id view.
        #[arg(long)]
        project: bool,
        /// All projects (default).
        #[arg(long)]
        global: bool,
        /// Filter by risk level: safe|review|danger.
        #[arg(long)]
        risk: Option<String>,
        /// Substring match on input or command.
        #[arg(long)]
        grep: Option<String>,
        /// JSON array on stdout.
        #[arg(long)]
        json: bool,
        /// JSON lines on stdout.
        #[arg(long)]
        jsonl: bool,
        #[command(subcommand)]
        cmd: Option<HistoryCmd>,
    },
    /// Named command macros (T-504). Never auto-run as `shx <name>`.
    Snippet {
        #[command(subcommand)]
        cmd: Option<SnippetCmd>,
    },
    /// Teach shorthand vocabulary (T-205).
    Teach {
        /// Term to forget.
        #[arg(long)]
        forget: Option<String>,
        /// List vocabulary.
        #[arg(long)]
        list: bool,
        /// JSON list.
        #[arg(long)]
        json: bool,
        /// term expansion.
        args: Vec<String>,
    },
    /// Record feedback on an interaction (T-503).
    Feedback {
        /// Interaction id.
        id: Option<i64>,
        /// good|bad.
        verdict: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        executed: bool,
        #[arg(long)]
        accepted: bool,
    },
    /// Inspect or edit configuration (T-007).
    Config {
        #[command(subcommand)]
        cmd: Option<ConfigCmd>,
    },
    /// Opt-in shell-history ingest (T-605).
    #[command(name = "import-history")]
    ImportHistory {
        #[arg(long)]
        shell: Option<String>,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Generate shell completions (T-604).
    Completion {
        /// zsh|bash|fish|powershell.
        shell: Option<String>,
    },
    /// Print the man page (T-604).
    Man,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum HistoryCmd {
    /// Newest-first list (default if no subcommand).
    List,
    /// Show one interaction.
    Show {
        id: i64,
        #[arg(long)]
        json: bool,
    },
    /// Dump history.
    Export {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        jsonl: bool,
        #[arg(long)]
        out: Option<String>,
    },
    /// Apply retention.
    Prune {
        /// e.g. 180d (days).
        #[arg(long)]
        older_than: Option<String>,
        #[arg(long)]
        keep_danger: bool,
    },
    /// Delete stored interactions.
    Purge {
        #[arg(long)]
        all: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum SnippetCmd {
    /// Store a named command macro (context for the model; never auto-executed).
    Save {
        /// Unique snippet name (e.g. pg-up).
        name: String,
        /// Command text to store (redacted at write).
        #[arg(long)]
        command: String,
        /// Optional description used when matching intent tokens.
        #[arg(short = 'd', long)]
        description: Option<String>,
    },
    /// List saved snippets (name + command).
    List {
        /// JSON array on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Show one snippet. `--copy` prints the command only (stdout).
    Show {
        name: String,
        /// Print the command to stdout (clipboard lands in T-603).
        #[arg(long)]
        copy: bool,
        /// JSON object on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Delete a snippet by name.
    Rm { name: String },
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum ConfigCmd {
    /// Print discovered config paths (lowest precedence first).
    Path,
    /// Print the effective merged config.
    Show {
        /// JSON object on stdout.
        #[arg(long)]
        json: bool,
    },
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
    },
    /// Open the global config in $EDITOR.
    Edit,
    /// Write a commented default config.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => return clap_exit(e),
    };

    let code = match dispatch(cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn clap_exit(err: clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    match err.kind() {
        ErrorKind::DisplayVersion => {
            print!("{err}");
            ExitCode::SUCCESS
        }
        ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            eprint!("{err}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{err}");
            ExitCode::from(EXIT_USAGE as u8)
        }
    }
}

fn dispatch(cli: Cli) -> anyhow::Result<i32> {
    if let Some(cmd) = &cli.command {
        return Ok(match cmd {
            Commands::Doctor {
                json,
                redaction_test,
            } => commands::doctor::run(*json, *redaction_test, &cli),
            Commands::Config { cmd } => commands::config::run(cmd.as_ref(), &cli),
            Commands::History { .. } => commands::history::run(&cli),
            Commands::Teach { .. } => commands::teach::run(&cli),
            Commands::Snippet { .. } => commands::snippet::run(&cli),
            Commands::Feedback { .. } => commands::feedback::run(&cli),
            Commands::ImportHistory { .. } => commands::import_history::run(&cli),
            Commands::Completion { .. } | Commands::Man => commands::completion::run(&cli),
        });
    }
    if cli.explain.is_some() {
        return Ok(commands::explain::run(&cli));
    }
    if cli.interactive {
        return Ok(commands::chat::run(&cli));
    }

    let flags = flag_overrides(&cli);
    let loaded = match load(flags) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("shx: {e}");
            return Ok(EXIT_USAGE);
        }
    };
    for w in &loaded.warnings {
        // Always stderr, including with --json (warnings are not the JSON object).
        eprintln!("warning: {w}");
    }

    match pipeline::run(&cli, &loaded.config, Vec::new(), None) {
        Ok(out) => Ok(render::render(
            &out,
            render::RenderOpts {
                json: cli.json,
                quiet: cli.quiet,
                verbose: cli.verbose,
                why: cli.why,
                exit_on_risk: loaded.config.safety.exit_on_risk,
                warn_on_risk: loaded.config.safety.warn_on_risk,
                color: render::color_stderr(loaded.config.ui.color),
                copy: cli.copy,
            },
        )),
        Err(PipelineError::Usage(msg)) => {
            eprintln!("shx: {msg}");
            Ok(EXIT_USAGE)
        }
        Err(PipelineError::Backend(msg)) => {
            eprintln!("shx: {msg}");
            Ok(EXIT_BACKEND)
        }
        Err(PipelineError::Other(msg)) => {
            eprintln!("shx: {msg}");
            Ok(EXIT_ERROR)
        }
    }
}
