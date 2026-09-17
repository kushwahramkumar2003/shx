# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_shx_global_optspecs
    string join \n n/count= json explain= i/interactive why local cloud offline copy exit-on-risk no-memory no-color profile= project= y/yes v/verbose q/quiet config= h/help V/version
end

function __fish_shx_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_shx_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_shx_using_subcommand
    set -l cmd (__fish_shx_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c shx -n "__fish_shx_needs_command" -s n -l count -d 'Return up to N candidates (one command per stdout line)' -r
complete -c shx -n "__fish_shx_needs_command" -l explain -d 'Reverse mode: explain an existing command (T-601)' -r
complete -c shx -n "__fish_shx_needs_command" -l profile -d 'Named context profile' -r
complete -c shx -n "__fish_shx_needs_command" -l project -d 'Override project scoping' -r
complete -c shx -n "__fish_shx_needs_command" -l config -d 'Alternate config file' -r
complete -c shx -n "__fish_shx_needs_command" -l json -d 'Structured JSON object on stdout'
complete -c shx -n "__fish_shx_needs_command" -s i -l interactive -d 'Refine session (T-602)'
complete -c shx -n "__fish_shx_needs_command" -l why -d 'Show which memory entries / routing were used (stderr)'
complete -c shx -n "__fish_shx_needs_command" -l local -d 'Force the local backend'
complete -c shx -n "__fish_shx_needs_command" -l cloud -d 'Force the cloud backend (error if unconfigured)'
complete -c shx -n "__fish_shx_needs_command" -l offline -d 'Forbid network; use mock/fixtures'
complete -c shx -n "__fish_shx_needs_command" -l copy -d 'Copy the result to the clipboard (T-603)'
complete -c shx -n "__fish_shx_needs_command" -l exit-on-risk -d 'Exit 3 if risk ≥ Review'
complete -c shx -n "__fish_shx_needs_command" -l no-memory -d 'Ignore memory for this call'
complete -c shx -n "__fish_shx_needs_command" -l no-color -d 'Disable ANSI on stderr'
complete -c shx -n "__fish_shx_needs_command" -s y -l yes -d 'Skip tool-side prompts (never execution)'
complete -c shx -n "__fish_shx_needs_command" -s v -l verbose -d 'Raw model output and full error chains on stderr'
complete -c shx -n "__fish_shx_needs_command" -s q -l quiet -d 'Suppress non-essential stderr'
complete -c shx -n "__fish_shx_needs_command" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_needs_command" -s V -l version -d 'Print version'
complete -c shx -n "__fish_shx_needs_command" -a "doctor" -d 'Self-check config, DB, and backends (T-007)'
complete -c shx -n "__fish_shx_needs_command" -a "history" -d 'Browse recorded translations (T-204)'
complete -c shx -n "__fish_shx_needs_command" -a "snippet" -d 'Named command macros (T-504). Never auto-run as `shx <name>`'
complete -c shx -n "__fish_shx_needs_command" -a "teach" -d 'Teach shorthand vocabulary (T-205)'
complete -c shx -n "__fish_shx_needs_command" -a "feedback" -d 'Record feedback on an interaction (T-503)'
complete -c shx -n "__fish_shx_needs_command" -a "config" -d 'Inspect or edit configuration (T-007)'
complete -c shx -n "__fish_shx_needs_command" -a "import-history" -d 'Opt-in shell-history ingest (T-605)'
complete -c shx -n "__fish_shx_needs_command" -a "completion" -d 'Generate shell completions (T-604)'
complete -c shx -n "__fish_shx_needs_command" -a "man" -d 'Print the man page (T-604)'
complete -c shx -n "__fish_shx_needs_command" -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand doctor" -l json -d 'JSON report'
complete -c shx -n "__fish_shx_using_subcommand doctor" -l redaction-test -d 'Run the redaction corpus and report per-pattern pass/fail'
complete -c shx -n "__fish_shx_using_subcommand doctor" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -l limit -d 'Max rows (list/export)' -r
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -l risk -d 'Filter by risk level: safe|review|danger' -r
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -l grep -d 'Substring match on input or command' -r
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -l project -d 'Restrict to the current project_id view'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -l global -d 'All projects (default)'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -l json -d 'JSON array on stdout'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -l jsonl -d 'JSON lines on stdout'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -f -a "list" -d 'Newest-first list (default if no subcommand)'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -f -a "show" -d 'Show one interaction'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -f -a "export" -d 'Dump history'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -f -a "prune" -d 'Apply retention'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -f -a "purge" -d 'Delete stored interactions'
complete -c shx -n "__fish_shx_using_subcommand history; and not __fish_seen_subcommand_from list show export prune purge help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from show" -l json
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from export" -l out -r
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from export" -l json
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from export" -l jsonl
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from export" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from prune" -l older-than -d 'e.g. 180d (days)' -r
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from prune" -l keep-danger
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from prune" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from purge" -l all
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from purge" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from help" -f -a "list" -d 'Newest-first list (default if no subcommand)'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from help" -f -a "show" -d 'Show one interaction'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from help" -f -a "export" -d 'Dump history'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from help" -f -a "prune" -d 'Apply retention'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from help" -f -a "purge" -d 'Delete stored interactions'
complete -c shx -n "__fish_shx_using_subcommand history; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and not __fish_seen_subcommand_from save list show rm help" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand snippet; and not __fish_seen_subcommand_from save list show rm help" -f -a "save" -d 'Store a named command macro (context for the model; never auto-executed)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and not __fish_seen_subcommand_from save list show rm help" -f -a "list" -d 'List saved snippets (name + command)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and not __fish_seen_subcommand_from save list show rm help" -f -a "show" -d 'Show one snippet. `--copy` prints the command only (stdout)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and not __fish_seen_subcommand_from save list show rm help" -f -a "rm" -d 'Delete a snippet by name'
complete -c shx -n "__fish_shx_using_subcommand snippet; and not __fish_seen_subcommand_from save list show rm help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from save" -l command -d 'Command text to store (redacted at write)' -r
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from save" -s d -l description -d 'Optional description used when matching intent tokens' -r
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from save" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from list" -l json -d 'JSON array on stdout'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from show" -l copy -d 'Print the command to stdout (clipboard lands in T-603)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from show" -l json -d 'JSON object on stdout'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from rm" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from help" -f -a "save" -d 'Store a named command macro (context for the model; never auto-executed)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from help" -f -a "list" -d 'List saved snippets (name + command)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from help" -f -a "show" -d 'Show one snippet. `--copy` prints the command only (stdout)'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from help" -f -a "rm" -d 'Delete a snippet by name'
complete -c shx -n "__fish_shx_using_subcommand snippet; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand teach" -l forget -d 'Term to forget' -r
complete -c shx -n "__fish_shx_using_subcommand teach" -l list -d 'List vocabulary'
complete -c shx -n "__fish_shx_using_subcommand teach" -l json -d 'JSON list'
complete -c shx -n "__fish_shx_using_subcommand teach" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand feedback" -l note -r
complete -c shx -n "__fish_shx_using_subcommand feedback" -l executed
complete -c shx -n "__fish_shx_using_subcommand feedback" -l accepted
complete -c shx -n "__fish_shx_using_subcommand feedback" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -f -a "path" -d 'Print discovered config paths (lowest precedence first)'
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -f -a "show" -d 'Print the effective merged config'
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -f -a "get"
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -f -a "set"
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -f -a "edit" -d 'Open the global config in $EDITOR'
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -f -a "init" -d 'Write a commented default config'
complete -c shx -n "__fish_shx_using_subcommand config; and not __fish_seen_subcommand_from path show get set edit init help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from path" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from show" -l json -d 'JSON object on stdout'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from get" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from set" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from edit" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from init" -l force -d 'Overwrite an existing file'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from init" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "path" -d 'Print discovered config paths (lowest precedence first)'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "show" -d 'Print the effective merged config'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "get"
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "set"
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "edit" -d 'Open the global config in $EDITOR'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "init" -d 'Write a commented default config'
complete -c shx -n "__fish_shx_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand import-history" -l shell -r
complete -c shx -n "__fish_shx_using_subcommand import-history" -l file -r
complete -c shx -n "__fish_shx_using_subcommand import-history" -l limit -r
complete -c shx -n "__fish_shx_using_subcommand import-history" -l dry-run
complete -c shx -n "__fish_shx_using_subcommand import-history" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand completion" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand man" -s h -l help -d 'Print help'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "doctor" -d 'Self-check config, DB, and backends (T-007)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "history" -d 'Browse recorded translations (T-204)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "snippet" -d 'Named command macros (T-504). Never auto-run as `shx <name>`'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "teach" -d 'Teach shorthand vocabulary (T-205)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "feedback" -d 'Record feedback on an interaction (T-503)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "config" -d 'Inspect or edit configuration (T-007)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "import-history" -d 'Opt-in shell-history ingest (T-605)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "completion" -d 'Generate shell completions (T-604)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "man" -d 'Print the man page (T-604)'
complete -c shx -n "__fish_shx_using_subcommand help; and not __fish_seen_subcommand_from doctor history snippet teach feedback config import-history completion man help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from history" -f -a "list" -d 'Newest-first list (default if no subcommand)'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from history" -f -a "show" -d 'Show one interaction'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from history" -f -a "export" -d 'Dump history'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from history" -f -a "prune" -d 'Apply retention'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from history" -f -a "purge" -d 'Delete stored interactions'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from snippet" -f -a "save" -d 'Store a named command macro (context for the model; never auto-executed)'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from snippet" -f -a "list" -d 'List saved snippets (name + command)'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from snippet" -f -a "show" -d 'Show one snippet. `--copy` prints the command only (stdout)'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from snippet" -f -a "rm" -d 'Delete a snippet by name'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "path" -d 'Print discovered config paths (lowest precedence first)'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "show" -d 'Print the effective merged config'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "get"
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "set"
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "edit" -d 'Open the global config in $EDITOR'
complete -c shx -n "__fish_shx_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "init" -d 'Write a commented default config'
