package main

import (
	"fmt"
	"io"
	"os"
)

const completionUsage = `Generate shell completion scripts for bash, zsh, or fish.

Usage:
  unic completion <bash|zsh|fish>

Setup:
  bash:  eval "$(unic completion bash)"
  zsh:   eval "$(unic completion zsh)"
  fish:  unic completion fish | source
`

func runCompletion(args []string, out io.Writer) error {
	if wantsHelp(args) {
		_, _ = fmt.Fprint(out, completionUsage)
		return nil
	}
	if len(args) != 1 {
		return usageError("usage: unic completion <bash|zsh|fish>")
	}
	switch args[0] {
	case "bash":
		_, _ = fmt.Fprint(out, bashCompletionScript)
		return nil
	case "zsh":
		_, _ = fmt.Fprint(out, zshCompletionScript)
		return nil
	case "fish":
		_, _ = fmt.Fprint(out, fishCompletionScript)
		return nil
	default:
		return usageError(fmt.Sprintf("unsupported shell %q (choose bash, zsh, or fish)", args[0]))
	}
}

func runCompletionStdout(args []string) error {
	return runCompletion(args, os.Stdout)
}

const bashCompletionScript = `# bash completion for unic
_unic_profiles() {
  unic profile list 2>/dev/null | awk 'NR>1 {print $1}'
}

_unic_completions() {
  local cur prev cmd sub
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"

  if [[ "$prev" == "-p" || "$prev" == "--profile" ]]; then
    COMPREPLY=( $(compgen -W "$(_unic_profiles)" -- "$cur") )
    return 0
  fi

  local top_cmds="init status daemon send recv history device config plugin migrate profile channel doctor completion dev help"
  local i=1
  while [[ $i -lt $COMP_CWORD ]]; do
    case "${COMP_WORDS[$i]}" in
      -p|--profile)
        ((i+=2))
        continue
        ;;
      --profile=*|-p=*|--json)
        ((i++))
        continue
        ;;
      *)
        if [[ -z "$cmd" ]]; then
          cmd="${COMP_WORDS[$i]}"
        elif [[ -z "$sub" ]]; then
          sub="${COMP_WORDS[$i]}"
        fi
        ((i++))
        ;;
    esac
  done

  if [[ -z "$cmd" ]]; then
    if [[ "$cur" == -* ]]; then
      COMPREPLY=( $(compgen -W "-p --profile --json -h --help --version" -- "$cur") )
    else
      COMPREPLY=( $(compgen -W "$top_cmds" -- "$cur") )
    fi
    return 0
  fi

  case "$cmd" in
    daemon)
      COMPREPLY=( $(compgen -W "start stop restart status logs help" -- "$cur") )
      ;;
    device)
      if [[ "$sub" == "suggest" ]]; then
        COMPREPLY=( $(compgen -W "list pair ignore restore" -- "$cur") )
      elif [[ "$sub" == "path" ]]; then
        COMPREPLY=( $(compgen -W "pause resume remove policy" -- "$cur") )
      else
        COMPREPLY=( $(compgen -W "add list addr approve reject rename remove path backends suggest help" -- "$cur") )
      fi
      ;;
    channel)
      COMPREPLY=( $(compgen -W "pair add pause resume remove help" -- "$cur") )
      ;;
    history|h)
      COMPREPLY=( $(compgen -W "list show send use delete tag expire export action help" -- "$cur") )
      ;;
    config)
      COMPREPLY=( $(compgen -W "path set apply --json" -- "$cur") )
      ;;
    plugin)
      COMPREPLY=( $(compgen -W "install instance list enable disable uninstall rollback settings help" -- "$cur") )
      ;;
    profile)
      COMPREPLY=( $(compgen -W "list path help" -- "$cur") )
      ;;
    completion)
      COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") )
      ;;
    dev)
      COMPREPLY=( $(compgen -W "watch capabilities submission cache logs diagnostics copy paste help" -- "$cur") )
      ;;
    init)
      COMPREPLY=( $(compgen -W "--name --fresh --json --help" -- "$cur") )
      ;;
    send|s)
      COMPREPLY=( $(compgen -W "--to -f --png --request-id --json --help" -- "$cur") )
      ;;
    recv|r)
      COMPREPLY=( $(compgen -W "list show dismiss --json --help" -- "$cur") )
      ;;
  esac
}

complete -F _unic_completions unic uniclip
`

const zshCompletionScript = `#compdef unic uniclip

_unic_profiles() {
  local -a profiles
  profiles=(${(f)"$(unic profile list 2>/dev/null | awk 'NR>1 {print $1}')"})
  _describe -t profiles 'profile' profiles
}

_unic() {
  local -a top_cmds
  top_cmds=(
    'init:Initialize a profile for first use'
    'status:Show profile, daemon, and peer status'
    'daemon:Start, stop, and inspect the background service'
    'send:Send text, files, or images'
    'recv:Receive from a code/link, or list received items'
    'history:Browse, show, restore, tag, and export history'
    'device:Pair, approve, and manage devices'
    'config:View and update settings'
    'plugin:Install and manage plugins'
    'migrate:Import UniClipboard v1 history'
    'profile:List and inspect local profiles'
    'channel:Pair and manage external clipboard channels'
    'doctor:Run diagnostics on the active profile and daemon'
    'completion:Generate shell completion scripts'
    'dev:Diagnostics, logs, and low-level tools'
    'help:Show help'
  )

  _arguments -C \
    '(-p --profile)'{-p,--profile}'[Use a named profile]:profile:_unic_profiles' \
    '--json[Output machine-readable JSON]' \
    '(-h --help)'{-h,--help}'[Show help]' \
    '--version[Show version]' \
    '1:command:->cmd' \
    '*::arg:->args'

  case "$state" in
    cmd)
      _describe -t commands 'unic command' top_cmds
      ;;
    args)
      case "$words[1]" in
        daemon)
          _values 'daemon command' start stop restart status logs help
          ;;
        device)
          _values 'device command' add list addr approve reject rename remove path backends suggest help
          ;;
        channel)
          _values 'channel command' pair add pause resume remove help
          ;;
        history|h)
          _values 'history command' list show send use delete tag expire export action help
          ;;
        config)
          _values 'config command' path set apply
          ;;
        plugin)
          _values 'plugin command' install instance list enable disable uninstall rollback settings help
          ;;
        profile)
          _values 'profile command' list path help
          ;;
        completion)
          _values 'shell' bash zsh fish
          ;;
        dev)
          _values 'dev command' watch capabilities submission cache logs diagnostics copy paste help
          ;;
      esac
      ;;
  esac
}

compdef _unic unic uniclip
`

const fishCompletionScript = `# fish completion for unic
function __unic_profiles
  unic profile list 2>/dev/null | awk 'NR>1 {print $1}'
end

complete -c unic -s p -l profile -d 'Use a named profile' -xa '(__unic_profiles)'
complete -c unic -l json -d 'Output machine-readable JSON'
complete -c unic -s h -l help -d 'Show help'
complete -c unic -l version -d 'Print version information'

complete -c unic -f -n '__fish_use_subcommand' -a init -d 'Initialize a profile for first use'
complete -c unic -f -n '__fish_use_subcommand' -a status -d 'Show profile, daemon, and peer status'
complete -c unic -f -n '__fish_use_subcommand' -a daemon -d 'Manage the background service'
complete -c unic -f -n '__fish_use_subcommand' -a send -d 'Send text, files, or images'
complete -c unic -f -n '__fish_use_subcommand' -a recv -d 'Receive from a code/link or list received items'
complete -c unic -f -n '__fish_use_subcommand' -a history -d 'Browse and manage history'
complete -c unic -f -n '__fish_use_subcommand' -a device -d 'Pair and manage devices'
complete -c unic -f -n '__fish_use_subcommand' -a config -d 'View and update settings'
complete -c unic -f -n '__fish_use_subcommand' -a plugin -d 'Install and manage plugins'
complete -c unic -f -n '__fish_use_subcommand' -a migrate -d 'Import UniClipboard v1 history'
complete -c unic -f -n '__fish_use_subcommand' -a profile -d 'List and inspect local profiles'
complete -c unic -f -n '__fish_use_subcommand' -a channel -d 'Pair and manage external channels'
complete -c unic -f -n '__fish_use_subcommand' -a doctor -d 'Run profile and daemon diagnostics'
complete -c unic -f -n '__fish_use_subcommand' -a completion -d 'Generate shell completions'
complete -c unic -f -n '__fish_use_subcommand' -a dev -d 'Development and diagnosis'
complete -c unic -f -n '__fish_use_subcommand' -a help -d 'Show help'

complete -c unic -f -n '__fish_seen_subcommand_from daemon' -a 'start stop restart status logs help'
complete -c unic -f -n '__fish_seen_subcommand_from device' -a 'add list addr approve reject rename remove path backends suggest help'
complete -c unic -f -n '__fish_seen_subcommand_from channel' -a 'pair add pause resume remove help'
complete -c unic -f -n '__fish_seen_subcommand_from history h' -a 'list show send use delete tag expire export action help'
complete -c unic -f -n '__fish_seen_subcommand_from config' -a 'path set apply'
complete -c unic -f -n '__fish_seen_subcommand_from plugin' -a 'install instance list enable disable uninstall rollback settings help'
complete -c unic -f -n '__fish_seen_subcommand_from profile' -a 'list path help'
complete -c unic -f -n '__fish_seen_subcommand_from completion' -a 'bash zsh fish'
complete -c unic -f -n '__fish_seen_subcommand_from dev' -a 'watch capabilities submission cache logs diagnostics copy paste help'
`
