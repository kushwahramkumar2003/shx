#compdef shx

autoload -U is-at-least

_shx() {
    typeset -A opt_args
    typeset -a _arguments_options
    local ret=1

    if is-at-least 5.2; then
        _arguments_options=(-s -S -C)
    else
        _arguments_options=(-s -C)
    fi

    local context curcontext="$curcontext" state line
    _arguments "${_arguments_options[@]}" : \
'-n+[Return up to N candidates (one command per stdout line)]:N:_default' \
'--count=[Return up to N candidates (one command per stdout line)]:N:_default' \
'--explain=[Reverse mode\: explain an existing command (T-601)]:CMD:_default' \
'--profile=[Named context profile]:NAME:_default' \
'--project=[Override project scoping]:PATH:_default' \
'--config=[Alternate config file]:PATH:_default' \
'--json[Structured JSON object on stdout]' \
'-i[Refine session (T-602)]' \
'--interactive[Refine session (T-602)]' \
'--why[Show which memory entries / routing were used (stderr)]' \
'(--cloud)--local[Force the local backend]' \
'--cloud[Force the cloud backend (error if unconfigured)]' \
'--offline[Forbid network; use mock/fixtures]' \
'--copy[Copy the result to the clipboard (T-603)]' \
'--exit-on-risk[Exit 3 if risk ≥ Review]' \
'--no-memory[Ignore memory for this call]' \
'--no-color[Disable ANSI on stderr]' \
'-y[Skip tool-side prompts (never execution)]' \
'--yes[Skip tool-side prompts (never execution)]' \
'-v[Raw model output and full error chains on stderr]' \
'--verbose[Raw model output and full error chains on stderr]' \
'-q[Suppress non-essential stderr]' \
'--quiet[Suppress non-essential stderr]' \
'-h[Print help]' \
'--help[Print help]' \
'-V[Print version]' \
'--version[Print version]' \
'::intent -- Natural-language intent (joined with spaces):_default' \
":: :_shx_commands" \
"*::: :->shx" \
&& ret=0
    case $state in
    (shx)
        words=($line[2] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-command-$line[2]:"
        case $line[2] in
            (doctor)
_arguments "${_arguments_options[@]}" : \
'--json[JSON report]' \
'--redaction-test[Run the redaction corpus and report per-pattern pass/fail]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(history)
_arguments "${_arguments_options[@]}" : \
'--limit=[Max rows (list/export)]:LIMIT:_default' \
'--risk=[Filter by risk level\: safe|review|danger]:RISK:_default' \
'--grep=[Substring match on input or command]:GREP:_default' \
'--project[Restrict to the current project_id view]' \
'--global[All projects (default)]' \
'--json[JSON array on stdout]' \
'--jsonl[JSON lines on stdout]' \
'-h[Print help]' \
'--help[Print help]' \
":: :_shx__subcmd__history_commands" \
"*::: :->history" \
&& ret=0

    case $state in
    (history)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-history-command-$line[1]:"
        case $line[1] in
            (list)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
'--json[]' \
'-h[Print help]' \
'--help[Print help]' \
':id:_default' \
&& ret=0
;;
(export)
_arguments "${_arguments_options[@]}" : \
'--out=[]:OUT:_default' \
'--json[]' \
'--jsonl[]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(prune)
_arguments "${_arguments_options[@]}" : \
'--older-than=[e.g. 180d (days)]:OLDER_THAN:_default' \
'--keep-danger[]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(purge)
_arguments "${_arguments_options[@]}" : \
'--all[]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_shx__subcmd__history__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-history-help-command-$line[1]:"
        case $line[1] in
            (list)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(export)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(prune)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(purge)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
;;
(snippet)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
":: :_shx__subcmd__snippet_commands" \
"*::: :->snippet" \
&& ret=0

    case $state in
    (snippet)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-snippet-command-$line[1]:"
        case $line[1] in
            (save)
_arguments "${_arguments_options[@]}" : \
'--command=[Command text to store (redacted at write)]:COMMAND:_default' \
'-d+[Optional description used when matching intent tokens]:DESCRIPTION:_default' \
'--description=[Optional description used when matching intent tokens]:DESCRIPTION:_default' \
'-h[Print help]' \
'--help[Print help]' \
':name -- Unique snippet name (e.g. pg-up):_default' \
&& ret=0
;;
(list)
_arguments "${_arguments_options[@]}" : \
'--json[JSON array on stdout]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
'--copy[Print the command to stdout (clipboard lands in T-603)]' \
'--json[JSON object on stdout]' \
'-h[Print help]' \
'--help[Print help]' \
':name:_default' \
&& ret=0
;;
(rm)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
':name:_default' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_shx__subcmd__snippet__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-snippet-help-command-$line[1]:"
        case $line[1] in
            (save)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(list)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(rm)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
;;
(teach)
_arguments "${_arguments_options[@]}" : \
'--forget=[Term to forget]:FORGET:_default' \
'--list[List vocabulary]' \
'--json[JSON list]' \
'-h[Print help]' \
'--help[Print help]' \
'*::args -- term expansion:_default' \
&& ret=0
;;
(feedback)
_arguments "${_arguments_options[@]}" : \
'--note=[]:NOTE:_default' \
'--executed[]' \
'--accepted[]' \
'-h[Print help]' \
'--help[Print help]' \
'::id -- Interaction id:_default' \
'::verdict -- good|bad:_default' \
&& ret=0
;;
(config)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
":: :_shx__subcmd__config_commands" \
"*::: :->config" \
&& ret=0

    case $state in
    (config)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-config-command-$line[1]:"
        case $line[1] in
            (path)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
'--json[JSON object on stdout]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(get)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
':key:_default' \
&& ret=0
;;
(set)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
':key:_default' \
':value:_default' \
&& ret=0
;;
(edit)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(init)
_arguments "${_arguments_options[@]}" : \
'--force[Overwrite an existing file]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_shx__subcmd__config__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-config-help-command-$line[1]:"
        case $line[1] in
            (path)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(get)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(set)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(edit)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(init)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
;;
(import-history)
_arguments "${_arguments_options[@]}" : \
'--shell=[]:SHELL:_default' \
'--file=[]:FILE:_default' \
'--limit=[]:LIMIT:_default' \
'--dry-run[]' \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(completion)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
'::shell -- zsh|bash|fish|powershell:_default' \
&& ret=0
;;
(man)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_shx__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-help-command-$line[1]:"
        case $line[1] in
            (doctor)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(history)
_arguments "${_arguments_options[@]}" : \
":: :_shx__subcmd__help__subcmd__history_commands" \
"*::: :->history" \
&& ret=0

    case $state in
    (history)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-help-history-command-$line[1]:"
        case $line[1] in
            (list)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(export)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(prune)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(purge)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
(snippet)
_arguments "${_arguments_options[@]}" : \
":: :_shx__subcmd__help__subcmd__snippet_commands" \
"*::: :->snippet" \
&& ret=0

    case $state in
    (snippet)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-help-snippet-command-$line[1]:"
        case $line[1] in
            (save)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(list)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(rm)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
(teach)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(feedback)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(config)
_arguments "${_arguments_options[@]}" : \
":: :_shx__subcmd__help__subcmd__config_commands" \
"*::: :->config" \
&& ret=0

    case $state in
    (config)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:shx-help-config-command-$line[1]:"
        case $line[1] in
            (path)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(show)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(get)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(set)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(edit)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(init)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
(import-history)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(completion)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(man)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
}

(( $+functions[_shx_commands] )) ||
_shx_commands() {
    local commands; commands=(
'doctor:Self-check config, DB, and backends (T-007)' \
'history:Browse recorded translations (T-204)' \
'snippet:Named command macros (T-504). Never auto-run as \`shx <name>\`' \
'teach:Teach shorthand vocabulary (T-205)' \
'feedback:Record feedback on an interaction (T-503)' \
'config:Inspect or edit configuration (T-007)' \
'import-history:Opt-in shell-history ingest (T-605)' \
'completion:Generate shell completions (T-604)' \
'man:Print the man page (T-604)' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx commands' commands "$@"
}
(( $+functions[_shx__subcmd__completion_commands] )) ||
_shx__subcmd__completion_commands() {
    local commands; commands=()
    _describe -t commands 'shx completion commands' commands "$@"
}
(( $+functions[_shx__subcmd__config_commands] )) ||
_shx__subcmd__config_commands() {
    local commands; commands=(
'path:Print discovered config paths (lowest precedence first)' \
'show:Print the effective merged config' \
'get:' \
'set:' \
'edit:Open the global config in \$EDITOR' \
'init:Write a commented default config' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx config commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__edit_commands] )) ||
_shx__subcmd__config__subcmd__edit_commands() {
    local commands; commands=()
    _describe -t commands 'shx config edit commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__get_commands] )) ||
_shx__subcmd__config__subcmd__get_commands() {
    local commands; commands=()
    _describe -t commands 'shx config get commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help_commands] )) ||
_shx__subcmd__config__subcmd__help_commands() {
    local commands; commands=(
'path:Print discovered config paths (lowest precedence first)' \
'show:Print the effective merged config' \
'get:' \
'set:' \
'edit:Open the global config in \$EDITOR' \
'init:Write a commented default config' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx config help commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help__subcmd__edit_commands] )) ||
_shx__subcmd__config__subcmd__help__subcmd__edit_commands() {
    local commands; commands=()
    _describe -t commands 'shx config help edit commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help__subcmd__get_commands] )) ||
_shx__subcmd__config__subcmd__help__subcmd__get_commands() {
    local commands; commands=()
    _describe -t commands 'shx config help get commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help__subcmd__help_commands] )) ||
_shx__subcmd__config__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'shx config help help commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help__subcmd__init_commands] )) ||
_shx__subcmd__config__subcmd__help__subcmd__init_commands() {
    local commands; commands=()
    _describe -t commands 'shx config help init commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help__subcmd__path_commands] )) ||
_shx__subcmd__config__subcmd__help__subcmd__path_commands() {
    local commands; commands=()
    _describe -t commands 'shx config help path commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help__subcmd__set_commands] )) ||
_shx__subcmd__config__subcmd__help__subcmd__set_commands() {
    local commands; commands=()
    _describe -t commands 'shx config help set commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__help__subcmd__show_commands] )) ||
_shx__subcmd__config__subcmd__help__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx config help show commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__init_commands] )) ||
_shx__subcmd__config__subcmd__init_commands() {
    local commands; commands=()
    _describe -t commands 'shx config init commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__path_commands] )) ||
_shx__subcmd__config__subcmd__path_commands() {
    local commands; commands=()
    _describe -t commands 'shx config path commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__set_commands] )) ||
_shx__subcmd__config__subcmd__set_commands() {
    local commands; commands=()
    _describe -t commands 'shx config set commands' commands "$@"
}
(( $+functions[_shx__subcmd__config__subcmd__show_commands] )) ||
_shx__subcmd__config__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx config show commands' commands "$@"
}
(( $+functions[_shx__subcmd__doctor_commands] )) ||
_shx__subcmd__doctor_commands() {
    local commands; commands=()
    _describe -t commands 'shx doctor commands' commands "$@"
}
(( $+functions[_shx__subcmd__feedback_commands] )) ||
_shx__subcmd__feedback_commands() {
    local commands; commands=()
    _describe -t commands 'shx feedback commands' commands "$@"
}
(( $+functions[_shx__subcmd__help_commands] )) ||
_shx__subcmd__help_commands() {
    local commands; commands=(
'doctor:Self-check config, DB, and backends (T-007)' \
'history:Browse recorded translations (T-204)' \
'snippet:Named command macros (T-504). Never auto-run as \`shx <name>\`' \
'teach:Teach shorthand vocabulary (T-205)' \
'feedback:Record feedback on an interaction (T-503)' \
'config:Inspect or edit configuration (T-007)' \
'import-history:Opt-in shell-history ingest (T-605)' \
'completion:Generate shell completions (T-604)' \
'man:Print the man page (T-604)' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx help commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__completion_commands] )) ||
_shx__subcmd__help__subcmd__completion_commands() {
    local commands; commands=()
    _describe -t commands 'shx help completion commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__config_commands] )) ||
_shx__subcmd__help__subcmd__config_commands() {
    local commands; commands=(
'path:Print discovered config paths (lowest precedence first)' \
'show:Print the effective merged config' \
'get:' \
'set:' \
'edit:Open the global config in \$EDITOR' \
'init:Write a commented default config' \
    )
    _describe -t commands 'shx help config commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__config__subcmd__edit_commands] )) ||
_shx__subcmd__help__subcmd__config__subcmd__edit_commands() {
    local commands; commands=()
    _describe -t commands 'shx help config edit commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__config__subcmd__get_commands] )) ||
_shx__subcmd__help__subcmd__config__subcmd__get_commands() {
    local commands; commands=()
    _describe -t commands 'shx help config get commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__config__subcmd__init_commands] )) ||
_shx__subcmd__help__subcmd__config__subcmd__init_commands() {
    local commands; commands=()
    _describe -t commands 'shx help config init commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__config__subcmd__path_commands] )) ||
_shx__subcmd__help__subcmd__config__subcmd__path_commands() {
    local commands; commands=()
    _describe -t commands 'shx help config path commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__config__subcmd__set_commands] )) ||
_shx__subcmd__help__subcmd__config__subcmd__set_commands() {
    local commands; commands=()
    _describe -t commands 'shx help config set commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__config__subcmd__show_commands] )) ||
_shx__subcmd__help__subcmd__config__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx help config show commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__doctor_commands] )) ||
_shx__subcmd__help__subcmd__doctor_commands() {
    local commands; commands=()
    _describe -t commands 'shx help doctor commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__feedback_commands] )) ||
_shx__subcmd__help__subcmd__feedback_commands() {
    local commands; commands=()
    _describe -t commands 'shx help feedback commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__help_commands] )) ||
_shx__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'shx help help commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__history_commands] )) ||
_shx__subcmd__help__subcmd__history_commands() {
    local commands; commands=(
'list:Newest-first list (default if no subcommand)' \
'show:Show one interaction' \
'export:Dump history' \
'prune:Apply retention' \
'purge:Delete stored interactions' \
    )
    _describe -t commands 'shx help history commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__history__subcmd__export_commands] )) ||
_shx__subcmd__help__subcmd__history__subcmd__export_commands() {
    local commands; commands=()
    _describe -t commands 'shx help history export commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__history__subcmd__list_commands] )) ||
_shx__subcmd__help__subcmd__history__subcmd__list_commands() {
    local commands; commands=()
    _describe -t commands 'shx help history list commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__history__subcmd__prune_commands] )) ||
_shx__subcmd__help__subcmd__history__subcmd__prune_commands() {
    local commands; commands=()
    _describe -t commands 'shx help history prune commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__history__subcmd__purge_commands] )) ||
_shx__subcmd__help__subcmd__history__subcmd__purge_commands() {
    local commands; commands=()
    _describe -t commands 'shx help history purge commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__history__subcmd__show_commands] )) ||
_shx__subcmd__help__subcmd__history__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx help history show commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__import-history_commands] )) ||
_shx__subcmd__help__subcmd__import-history_commands() {
    local commands; commands=()
    _describe -t commands 'shx help import-history commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__man_commands] )) ||
_shx__subcmd__help__subcmd__man_commands() {
    local commands; commands=()
    _describe -t commands 'shx help man commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__snippet_commands] )) ||
_shx__subcmd__help__subcmd__snippet_commands() {
    local commands; commands=(
'save:Store a named command macro (context for the model; never auto-executed)' \
'list:List saved snippets (name + command)' \
'show:Show one snippet. \`--copy\` prints the command only (stdout)' \
'rm:Delete a snippet by name' \
    )
    _describe -t commands 'shx help snippet commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__snippet__subcmd__list_commands] )) ||
_shx__subcmd__help__subcmd__snippet__subcmd__list_commands() {
    local commands; commands=()
    _describe -t commands 'shx help snippet list commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__snippet__subcmd__rm_commands] )) ||
_shx__subcmd__help__subcmd__snippet__subcmd__rm_commands() {
    local commands; commands=()
    _describe -t commands 'shx help snippet rm commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__snippet__subcmd__save_commands] )) ||
_shx__subcmd__help__subcmd__snippet__subcmd__save_commands() {
    local commands; commands=()
    _describe -t commands 'shx help snippet save commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__snippet__subcmd__show_commands] )) ||
_shx__subcmd__help__subcmd__snippet__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx help snippet show commands' commands "$@"
}
(( $+functions[_shx__subcmd__help__subcmd__teach_commands] )) ||
_shx__subcmd__help__subcmd__teach_commands() {
    local commands; commands=()
    _describe -t commands 'shx help teach commands' commands "$@"
}
(( $+functions[_shx__subcmd__history_commands] )) ||
_shx__subcmd__history_commands() {
    local commands; commands=(
'list:Newest-first list (default if no subcommand)' \
'show:Show one interaction' \
'export:Dump history' \
'prune:Apply retention' \
'purge:Delete stored interactions' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx history commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__export_commands] )) ||
_shx__subcmd__history__subcmd__export_commands() {
    local commands; commands=()
    _describe -t commands 'shx history export commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__help_commands] )) ||
_shx__subcmd__history__subcmd__help_commands() {
    local commands; commands=(
'list:Newest-first list (default if no subcommand)' \
'show:Show one interaction' \
'export:Dump history' \
'prune:Apply retention' \
'purge:Delete stored interactions' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx history help commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__help__subcmd__export_commands] )) ||
_shx__subcmd__history__subcmd__help__subcmd__export_commands() {
    local commands; commands=()
    _describe -t commands 'shx history help export commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__help__subcmd__help_commands] )) ||
_shx__subcmd__history__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'shx history help help commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__help__subcmd__list_commands] )) ||
_shx__subcmd__history__subcmd__help__subcmd__list_commands() {
    local commands; commands=()
    _describe -t commands 'shx history help list commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__help__subcmd__prune_commands] )) ||
_shx__subcmd__history__subcmd__help__subcmd__prune_commands() {
    local commands; commands=()
    _describe -t commands 'shx history help prune commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__help__subcmd__purge_commands] )) ||
_shx__subcmd__history__subcmd__help__subcmd__purge_commands() {
    local commands; commands=()
    _describe -t commands 'shx history help purge commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__help__subcmd__show_commands] )) ||
_shx__subcmd__history__subcmd__help__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx history help show commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__list_commands] )) ||
_shx__subcmd__history__subcmd__list_commands() {
    local commands; commands=()
    _describe -t commands 'shx history list commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__prune_commands] )) ||
_shx__subcmd__history__subcmd__prune_commands() {
    local commands; commands=()
    _describe -t commands 'shx history prune commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__purge_commands] )) ||
_shx__subcmd__history__subcmd__purge_commands() {
    local commands; commands=()
    _describe -t commands 'shx history purge commands' commands "$@"
}
(( $+functions[_shx__subcmd__history__subcmd__show_commands] )) ||
_shx__subcmd__history__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx history show commands' commands "$@"
}
(( $+functions[_shx__subcmd__import-history_commands] )) ||
_shx__subcmd__import-history_commands() {
    local commands; commands=()
    _describe -t commands 'shx import-history commands' commands "$@"
}
(( $+functions[_shx__subcmd__man_commands] )) ||
_shx__subcmd__man_commands() {
    local commands; commands=()
    _describe -t commands 'shx man commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet_commands] )) ||
_shx__subcmd__snippet_commands() {
    local commands; commands=(
'save:Store a named command macro (context for the model; never auto-executed)' \
'list:List saved snippets (name + command)' \
'show:Show one snippet. \`--copy\` prints the command only (stdout)' \
'rm:Delete a snippet by name' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx snippet commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__help_commands] )) ||
_shx__subcmd__snippet__subcmd__help_commands() {
    local commands; commands=(
'save:Store a named command macro (context for the model; never auto-executed)' \
'list:List saved snippets (name + command)' \
'show:Show one snippet. \`--copy\` prints the command only (stdout)' \
'rm:Delete a snippet by name' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'shx snippet help commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__help__subcmd__help_commands] )) ||
_shx__subcmd__snippet__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet help help commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__help__subcmd__list_commands] )) ||
_shx__subcmd__snippet__subcmd__help__subcmd__list_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet help list commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__help__subcmd__rm_commands] )) ||
_shx__subcmd__snippet__subcmd__help__subcmd__rm_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet help rm commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__help__subcmd__save_commands] )) ||
_shx__subcmd__snippet__subcmd__help__subcmd__save_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet help save commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__help__subcmd__show_commands] )) ||
_shx__subcmd__snippet__subcmd__help__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet help show commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__list_commands] )) ||
_shx__subcmd__snippet__subcmd__list_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet list commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__rm_commands] )) ||
_shx__subcmd__snippet__subcmd__rm_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet rm commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__save_commands] )) ||
_shx__subcmd__snippet__subcmd__save_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet save commands' commands "$@"
}
(( $+functions[_shx__subcmd__snippet__subcmd__show_commands] )) ||
_shx__subcmd__snippet__subcmd__show_commands() {
    local commands; commands=()
    _describe -t commands 'shx snippet show commands' commands "$@"
}
(( $+functions[_shx__subcmd__teach_commands] )) ||
_shx__subcmd__teach_commands() {
    local commands; commands=()
    _describe -t commands 'shx teach commands' commands "$@"
}

if [ "$funcstack[1]" = "_shx" ]; then
    _shx "$@"
else
    compdef _shx shx
fi
