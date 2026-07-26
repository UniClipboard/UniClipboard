#!/usr/bin/env bash
# sync-to-win.sh — Push the macOS working-tree changes to a remote Windows repo
# using rsync over SSH.
#
# This replaces the old SMB-mount strategy. The Windows machine must run an
# SSH server (built-in OpenSSH on modern Windows) and have `rsync` available
# in the user's PATH (typically via Git for Windows / MSYS2 / Cygwin / WSL).
#
# Configuration:
#   1. Copy config.example.sh -> config.local.sh in the same directory.
#   2. Fill in WIN_HOST / WIN_USER / WIN_REPO and either WIN_PASS or WIN_KEY.
#   3. Run `./sync-to-win.sh config` to verify what got loaded.
#
# Strategy:
#   1. Both sides must be at the same git HEAD (otherwise we'd silently
#      overwrite a different commit on the win side).
#   2. The win working tree must be clean (or pass --force to reset it first).
#   3. rsync the set of files that mac considers "tracked + untracked-not-
#      ignored" onto win. Files deleted in the mac working tree are removed
#      on win via an explicit ssh+rm pass.
#
# We never touch .git/ on the win side, never modify win HEAD, never push
# commits. This is a working-tree mirror — the win repo stays "owned" by git,
# we just borrow its working tree.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
CONFIG_FILE="${SYNC_CONFIG:-$SCRIPT_DIR/config.local.sh}"

# --- defaults (overridable via config.local.sh or env) ---------------------

MAC_REPO="${MAC_REPO:-/Volumes/ExternalSSD/superset/uniclipboard/slender-soybean}"
WIN_HOST="${WIN_HOST:-}"
WIN_PORT="${WIN_PORT:-22}"
WIN_USER="${WIN_USER:-}"
WIN_PASS="${WIN_PASS:-}"
WIN_KEY="${WIN_KEY:-}"
WIN_REPO="${WIN_REPO:-}"
EXTRA_EXCLUDES=()

if [[ -f "$CONFIG_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$CONFIG_FILE"
fi

# Always-excluded paths. Keep .git out (we mirror working tree only) and skip
# heavy build artifact dirs that are never useful to push.
DEFAULT_EXCLUDES=(
  ".git/"
  "target/"
  "node_modules/"
  "dist/"
  "dist-ssr/"
  ".DS_Store"
)

usage() {
  cat <<'EOF'
Usage: sync-to-win.sh <command> [options]

Commands:
  config            Print the resolved configuration (host, user, port, paths,
                    auth mode). Run after editing config.local.sh.
  check             Verify connectivity, remote git repo, and remote rsync.
  probe             Detect the win side's default shell, rsync, bash, git,
                    winget, scoop. Use this when `check` fails to figure out
                    what's actually installed.
  install-rsync     Install rsync on the win side via scoop (user-scope, no
                    admin). Requires scoop to be already installed on win.
  status            Show both sides: branch, HEAD, dirty files.
  diff              Show what would be synced (tracked diffstat + untracked list).
  push              rsync mac working-tree files to win. Pure file diff —
                    does not check git HEAD alignment or win's dirty state.
        --dry-run   Show rsync's plan without modifying the win repo.
  reset             git reset --hard + clean on win
                    (preserves target/ and node_modules/ to avoid a rebuild).
  ssh [args...]     Open an interactive ssh session, or run a remote command.
  paths             Print MAC_REPO / WIN_REPO and the remote endpoint.

Configuration:
  Copy config.example.sh -> config.local.sh in this directory and fill in:
    WIN_HOST, WIN_USER, WIN_REPO, plus either WIN_PASS or WIN_KEY.
EOF
}

# --- helpers ---------------------------------------------------------------

err() { printf '✗ %s\n' "$*" >&2; }
log() { printf '→ %s\n' "$*"; }
ok()  { printf '✓ %s\n' "$*"; }

require_config() {
  local missing=()
  [[ -n "$WIN_HOST" ]] || missing+=("WIN_HOST")
  [[ -n "$WIN_USER" ]] || missing+=("WIN_USER")
  [[ -n "$WIN_REPO" ]] || missing+=("WIN_REPO")
  if (( ${#missing[@]} )); then
    err "missing config: ${missing[*]}"
    echo "  edit:  $CONFIG_FILE" >&2
    if [[ ! -f "$CONFIG_FILE" ]]; then
      echo "  init:  cp '$SCRIPT_DIR/config.example.sh' '$CONFIG_FILE'" >&2
    fi
    exit 1
  fi
  if [[ -n "$WIN_PASS" ]] && ! command -v sshpass >/dev/null 2>&1; then
    err "WIN_PASS is set but sshpass is not installed."
    echo "  install: brew install hudochenkov/sshpass/sshpass" >&2
    echo "  or switch to key auth (clear WIN_PASS, set WIN_KEY or use ssh-agent)." >&2
    exit 1
  fi
  if [[ -n "$WIN_KEY" && ! -f "$WIN_KEY" ]]; then
    err "WIN_KEY points to a missing file: $WIN_KEY"
    exit 1
  fi
}

# Build the SSH command (as an array we can pass to ssh / rsync's -e).
ssh_command_string() {
  local cmd="ssh -p $WIN_PORT"
  cmd+=" -o StrictHostKeyChecking=accept-new"
  cmd+=" -o ConnectTimeout=10"
  cmd+=" -o ServerAliveInterval=15"
  if [[ -n "$WIN_KEY" ]]; then
    cmd+=" -i '$WIN_KEY' -o IdentitiesOnly=yes"
  fi
  if [[ -n "$WIN_PASS" ]]; then
    # sshpass wraps ssh and feeds the password on stdin.
    cmd="sshpass -p '$WIN_PASS' $cmd"
  fi
  printf '%s' "$cmd"
}

ssh_run() {
  # Run a command on the remote. Args are joined into a single remote command.
  local rcmd="$*"
  if [[ -n "$WIN_PASS" ]]; then
    sshpass -p "$WIN_PASS" ssh -p "$WIN_PORT" \
      -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 \
      ${WIN_KEY:+-i "$WIN_KEY" -o IdentitiesOnly=yes} \
      "$WIN_USER@$WIN_HOST" "$rcmd"
  else
    ssh -p "$WIN_PORT" \
      -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 \
      ${WIN_KEY:+-i "$WIN_KEY" -o IdentitiesOnly=yes} \
      "$WIN_USER@$WIN_HOST" "$rcmd"
  fi
}

# Run rsync with the right -e wrapper and standard flags.
# Caller passes additional rsync args after.
rsync_run() {
  local rsh
  rsh="$(ssh_command_string)"
  rsync -e "$rsh" "$@"
}

g_mac() { git -C "$MAC_REPO" "$@"; }
g_win() { ssh_run "git -C \"$WIN_REPO\" $*"; }

require_local_repo() {
  [[ -e "$MAC_REPO/.git" ]] && git -C "$MAC_REPO" rev-parse --git-dir >/dev/null 2>&1 \
    || { err "MAC_REPO is not a git repo: $MAC_REPO"; exit 1; }
}

# --- commands --------------------------------------------------------------

cmd_config() {
  echo "config file:   $CONFIG_FILE $( [[ -f "$CONFIG_FILE" ]] && echo '(present)' || echo '(missing)' )"
  echo "MAC_REPO:      $MAC_REPO"
  echo "WIN_HOST:      ${WIN_HOST:-(unset)}"
  echo "WIN_PORT:      $WIN_PORT"
  echo "WIN_USER:      ${WIN_USER:-(unset)}"
  echo "WIN_REPO:      ${WIN_REPO:-(unset)}"
  if [[ -n "$WIN_PASS" ]]; then
    echo "auth:          password (sshpass)"
  elif [[ -n "$WIN_KEY" ]]; then
    echo "auth:          key file ($WIN_KEY)"
  else
    echo "auth:          ssh-agent / default keys"
  fi
  if (( ${#EXTRA_EXCLUDES[@]} )); then
    echo "extra excludes: ${EXTRA_EXCLUDES[*]}"
  fi
}

# Run a PowerShell snippet on the remote via -EncodedCommand. Pipes the script
# through iconv → base64 to avoid every cmd.exe / ssh quoting trap. Output is
# UTF-8 (set inside the snippet) and CRLF-stripped here.
ps_run() {
  local snippet="$1"
  local b64
  b64=$(printf '%s\n%s' '[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8; $ProgressPreference = "SilentlyContinue"' "$snippet" \
        | iconv -t UTF-16LE | base64 | tr -d '\n')
  # Capture first, transform second. Piping ssh_run's stdout directly into tr
  # interacts badly with sshpass + no-tty in some configs (auth fails).
  # `|| true` so a non-zero remote exit (e.g. scoop install failure) doesn't
  # abort the wrapping bash script under set -e — caller checks output content.
  local raw
  raw=$(ssh_run "powershell -NoProfile -EncodedCommand $b64") || true
  printf '%s' "$raw" | LC_ALL=C tr -d '\r'
}

cmd_check() {
  require_config
  require_local_repo
  log "ssh login → $WIN_USER@$WIN_HOST:$WIN_PORT"
  if ! ssh_run "echo ok" >/dev/null 2>&1; then
    err "ssh login failed. Check WIN_HOST/WIN_PORT/WIN_USER and credentials."
    err "Run manually:  ssh -p $WIN_PORT $WIN_USER@$WIN_HOST"
    exit 1
  fi
  ok "ssh reachable"

  log "remote rsync presence"
  local rsync_path
  rsync_path=$(ps_run '(Get-Command rsync -EA SilentlyContinue).Source')
  if [[ -z "$rsync_path" ]]; then
    err "rsync not found on the win side."
    err "Run:  ./sync-to-win.sh install-rsync   (uses scoop, user-scope, no admin)"
    exit 1
  fi
  ok "remote rsync at: $rsync_path"

  log "remote git repo at $WIN_REPO"
  local head
  head=$(ps_run "if (Test-Path '$WIN_REPO/.git') { (git -C '$WIN_REPO' rev-parse HEAD) }")
  if [[ -z "$head" ]]; then
    err "$WIN_REPO is not a git repo on the win side (or path is wrong)."
    exit 1
  fi
  ok "remote git repo at HEAD $head"
}

cmd_install_rsync() {
  require_config
  # Strategy: portable rsync from rn7s2/rsync-win (cygwin-built rsync.exe +
  # bundled DLLs in a single zip). Avoids the scoop bucket/manifest fragility
  # we hit before, no admin or package manager needed.
  local pscmd
  pscmd='
$ErrorActionPreference = "Stop"
$url  = "https://github.com/rn7s2/rsync-win/releases/latest/download/rsync-win.zip"
$dest = Join-Path $env:USERPROFILE "rsync-win"
$zip  = Join-Path $env:TEMP "rsync-win.zip"

Write-Output "Downloading $url ..."
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

if (Test-Path $dest) {
  Write-Output "Removing previous install at $dest ..."
  Remove-Item -Recurse -Force $dest
}
Write-Output "Extracting to $dest ..."
Expand-Archive -Path $zip -DestinationPath $dest -Force
Remove-Item $zip -Force

# cygwin64\ contains rsync.exe, ssh.exe, cygwin1.dll, etc. Putting that
# directory on PATH makes rsync.exe runnable (it loads its sibling DLLs).
$rsyncDir = Join-Path $dest "cygwin64"
if (-not (Test-Path (Join-Path $rsyncDir "rsync.exe"))) {
  throw "rsync.exe missing after extraction (expected at $rsyncDir\rsync.exe)"
}

# Persist to user PATH (HKCU). New ssh logins will pick this up; the current
# ssh sessions PATH is already frozen and we deliberately do not touch it.
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not $userPath) { $userPath = "" }
if ($userPath -notlike "*$rsyncDir*") {
  $newPath = if ($userPath) { "$userPath;$rsyncDir" } else { $rsyncDir }
  [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
  Write-Output "Added $rsyncDir to user PATH."
} else {
  Write-Output "$rsyncDir already on user PATH."
}

Write-Output "RSYNC_PATH=$rsyncDir\rsync.exe"
& "$rsyncDir\rsync.exe" --version | Select-Object -First 1
'

  log "downloading + extracting rn7s2/rsync-win on the win side…"
  local out
  out=$(ps_run "$pscmd")
  printf '%s\n' "$out" | sed 's/^/    /'

  if ! printf '%s' "$out" | grep -q 'RSYNC_PATH='; then
    err "install failed — output above. Check network access to github.com from win."
    exit 1
  fi

  log "verifying rsync is on PATH for a NEW ssh session…"
  # Open a fresh ssh connection — that picks up the just-updated user PATH.
  local rsync_path
  rsync_path=$(ps_run '(Get-Command rsync -EA SilentlyContinue).Source')
  if [[ -z "$rsync_path" ]]; then
    err "rsync still not on PATH in a new session. The PATH update is in HKCU\\Environment;"
    err "you may need to log out + back in on the win side once for OpenSSH to pick it up."
    err "Workaround: every push still works if you pass the full rsync path"
    err "via --rsync-path on rsync calls. The exact path is in the output above"
    err "(line starting with 'RSYNC_PATH=')."
    exit 1
  fi
  ok "rsync at: $rsync_path"
  ok "now run:  ./sync-to-win.sh check"
}

cmd_probe() {
  require_config
  echo "=== probing $WIN_USER@$WIN_HOST:$WIN_PORT ==="
  echo "(single ssh call, powershell -EncodedCommand)"
  echo

  # PowerShell script: emits labelled lines so we can parse locally.
  # We send it via -EncodedCommand (base64 of UTF-16LE) to avoid every
  # quoting / line-buffering trap between ssh, cmd.exe, and powershell.
  local pscmd
  pscmd='
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
$ds = (Get-ItemProperty HKLM:\SOFTWARE\OpenSSH DefaultShell -EA SilentlyContinue).DefaultShell
"DefaultShell=$ds"
foreach ($t in "rsync","bash","git","winget","scoop") {
  $p = (Get-Command $t -EA SilentlyContinue).Source
  "Tool=$t=$p"
}
"Path=$($env:Path)"
'
  local b64
  b64=$(printf '%s' "$pscmd" | iconv -t UTF-16LE | base64 | tr -d '\n')

  local out
  if ! out=$(ssh_run "powershell -NoProfile -EncodedCommand $b64" 2>&1); then
    err "remote probe failed:"
    printf '%s\n' "$out" | sed 's/^/  /' >&2
    exit 1
  fi
  # LC_ALL=C makes BSD tr byte-clean in case any non-UTF8 sneaks through.
  out=$(printf '%s' "$out" | LC_ALL=C tr -d '\r')

  # Pretty-print.
  while IFS= read -r line; do
    case "$line" in
      DefaultShell=*)
        local v="${line#DefaultShell=}"
        if [[ -z "$v" ]]; then
          printf '  %-20s : (unset → cmd.exe)\n' "OpenSSH DefaultShell"
        else
          printf '  %-20s : %s\n' "OpenSSH DefaultShell" "$v"
        fi ;;
      Tool=*)
        local rest="${line#Tool=}"
        local name="${rest%%=*}"
        local path="${rest#*=}"
        if [[ -z "$path" ]]; then
          printf '  %-20s : (not found)\n' "$name"
        else
          printf '  %-20s : %s\n' "$name" "$path"
        fi ;;
      Path=*)
        echo
        echo "PATH (first 15 entries):"
        printf '%s' "${line#Path=}" | tr ';' '\n' | sed '/^$/d' | head -15 | sed 's/^/    /'
        ;;
    esac
  done <<<"$out"
}

cmd_status() {
  require_config
  require_local_repo
  local mh wh mb wb md wd
  mh=$(g_mac rev-parse HEAD)
  mb=$(g_mac branch --show-current)
  md=$(g_mac status --short)

  echo "=== mac (source) ==="
  echo "  path:   $MAC_REPO"
  echo "  branch: $mb"
  echo "  HEAD:   $mh"
  if [[ -n "$md" ]]; then
    echo "  dirty:"
    printf '%s\n' "$md" | sed 's/^/    /'
  else
    echo "  dirty:  (clean)"
  fi
  echo

  echo "=== win (target via ssh) ==="
  echo "  endpoint: $WIN_USER@$WIN_HOST:$WIN_PORT"
  echo "  path:     $WIN_REPO"
  if ! wh=$(ssh_run "git -C \"$WIN_REPO\" rev-parse HEAD" 2>/dev/null); then
    err "  unable to reach remote repo (run: ./sync-to-win.sh check)"
    return 1
  fi
  wh=$(printf '%s' "$wh" | tr -d '\r\n ')
  wb=$(ssh_run "git -C \"$WIN_REPO\" branch --show-current" 2>/dev/null | tr -d '\r' || echo "(unknown)")
  wd=$(ssh_run "git -C \"$WIN_REPO\" status --short" 2>/dev/null | tr -d '\r' || true)
  echo "  branch:   $wb"
  echo "  HEAD:     $wh"
  if [[ -n "$wd" ]]; then
    echo "  dirty:"
    printf '%s\n' "$wd" | sed 's/^/    /'
  else
    echo "  dirty:    (clean)"
  fi
  echo
  if [[ "$mh" == "$wh" ]]; then
    ok "HEAD aligned"
  else
    err "HEAD mismatch — sync refuses until aligned (git push/pull both repos to the same commit)"
  fi
  if [[ -n "$wb" && "$mb" != "$wb" ]]; then
    echo "! branch differs (mac=$mb, win=$wb) — usually fine but worth noting"
  fi
}

cmd_diff() {
  require_config
  require_local_repo
  echo "--- tracked changes (mac vs HEAD) ---"
  g_mac diff HEAD --stat || true
  echo
  echo "--- untracked-not-ignored files (would be copied) ---"
  local untracked
  untracked=$(g_mac ls-files --others --exclude-standard)
  if [[ -n "$untracked" ]]; then
    printf '%s\n' "$untracked"
  else
    echo "(none)"
  fi
  echo
  echo "--- deletions (would be removed on win) ---"
  local deleted
  deleted=$(g_mac ls-files --deleted)
  if [[ -n "$deleted" ]]; then
    printf '%s\n' "$deleted"
  else
    echo "(none)"
  fi
}

cmd_push() {
  require_config
  require_local_repo

  local dry=0
  for a in "$@"; do
    case "$a" in
      --dry-run) dry=1;;
      --force)   ;;  # accepted for backwards compat; no longer needed
      *) err "unknown arg: $a"; exit 2;;
    esac
  done

  # No git-state gating. We use git only to compute *which* files are part of
  # the working set on mac (tracked + untracked-not-ignored, minus deletions).
  # The win side's HEAD / dirty state is irrelevant — rsync just brings its
  # files into line with mac's.

  # Build the file lists.
  #
  # `to_send`: files that exist on disk in mac and that git thinks are part of
  #   the working set (tracked OR untracked-not-ignored). `--deduplicate`
  #   prevents double entries.
  # `to_delete`: tracked files that are missing on mac. We rm these on win so
  #   the sides stay aligned.
  local tmp_send tmp_del
  tmp_send=$(mktemp -t sync-to-win.send.XXXXXX)
  tmp_del=$(mktemp -t sync-to-win.del.XXXXXX)
  trap "rm -f '$tmp_send' '$tmp_del'" EXIT

  # Filter to entries that actually exist on disk; rsync would error otherwise.
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    [[ -e "$MAC_REPO/$f" ]] && printf '%s\n' "$f"
  done < <(g_mac ls-files --cached --others --exclude-standard --deduplicate) > "$tmp_send"

  g_mac ls-files --deleted > "$tmp_del" || true

  local n_send n_del
  n_send=$(wc -l < "$tmp_send" | tr -d ' ')
  n_del=$(wc -l < "$tmp_del" | tr -d ' ')

  # rsync
  local -a rsync_args
  rsync_args=(
    --archive
    --human-readable
    --itemize-changes
    --files-from="$tmp_send"
    --no-implied-dirs
  )
  for ex in "${DEFAULT_EXCLUDES[@]}" ${EXTRA_EXCLUDES[@]+"${EXTRA_EXCLUDES[@]}"}; do
    rsync_args+=("--exclude=$ex")
  done
  if [[ $dry -eq 1 ]]; then
    rsync_args+=("--dry-run")
  fi
  rsync_args+=("$MAC_REPO/" "$WIN_USER@$WIN_HOST:$WIN_REPO/")

  if (( n_send > 0 )); then
    if [[ $dry -eq 1 ]]; then
      echo "[dry-run] would rsync $n_send files to win"
    else
      log "rsyncing $n_send files to win…"
    fi
    rsync_run "${rsync_args[@]}"
  else
    log "no files to rsync"
  fi

  # Apply deletions on win.
  if (( n_del > 0 )); then
    if [[ $dry -eq 1 ]]; then
      echo "[dry-run] would remove $n_del files on win:"
      sed 's/^/    - /' "$tmp_del"
    else
      log "removing $n_del deleted files on win…"
      # Build a single shell command: cd && rm -f -- f1 f2 ...
      # Quote each path safely for the remote shell.
      local rm_cmd="cd \"$WIN_REPO\" && rm -f --"
      while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        # Single-quote escape: ' -> '\''
        local q="'${f//\'/\'\\\'\'}'"
        rm_cmd+=" $q"
      done < "$tmp_del"
      ssh_run "$rm_cmd"
    fi
  fi

  if [[ $dry -eq 0 ]]; then
    echo
    ok "done. win status:"
    ssh_run "git -C \"$WIN_REPO\" status --short" | tr -d '\r' | head -20 | sed 's/^/    /'
  fi
}

cmd_reset() {
  require_config
  log "git reset --hard HEAD on win"
  ssh_run "git -C \"$WIN_REPO\" reset --hard HEAD" >/dev/null
  log "git clean -fd on win (preserving target/ node_modules/)"
  ssh_run "git -C \"$WIN_REPO\" clean -fd -- ':!target' ':!node_modules'" >/dev/null || true
  ok "win is clean"
  ssh_run "git -C \"$WIN_REPO\" status --short" | tr -d '\r' | head -10 | sed 's/^/    /'
}

cmd_ssh() {
  require_config
  if [[ $# -eq 0 ]]; then
    # Interactive shell.
    if [[ -n "$WIN_PASS" ]]; then
      sshpass -p "$WIN_PASS" ssh -p "$WIN_PORT" \
        -o StrictHostKeyChecking=accept-new \
        ${WIN_KEY:+-i "$WIN_KEY" -o IdentitiesOnly=yes} \
        "$WIN_USER@$WIN_HOST"
    else
      ssh -p "$WIN_PORT" \
        -o StrictHostKeyChecking=accept-new \
        ${WIN_KEY:+-i "$WIN_KEY" -o IdentitiesOnly=yes} \
        "$WIN_USER@$WIN_HOST"
    fi
  else
    ssh_run "$*"
  fi
}

cmd_paths() {
  echo "MAC_REPO=$MAC_REPO"
  echo "WIN_REPO=$WIN_REPO"
  echo "remote=$WIN_USER@$WIN_HOST:$WIN_PORT"
}

# --- dispatch --------------------------------------------------------------

cmd="${1:-}"; shift || true
case "$cmd" in
  config)        cmd_config "$@";;
  check)         cmd_check  "$@";;
  probe)         cmd_probe  "$@";;
  install-rsync) cmd_install_rsync "$@";;
  status)        cmd_status "$@";;
  diff)   cmd_diff   "$@";;
  push)   cmd_push   "$@";;
  reset)  cmd_reset  "$@";;
  ssh)    cmd_ssh    "$@";;
  paths)  cmd_paths  "$@";;
  -h|--help|help|"") usage;;
  *) err "unknown command: $cmd"; usage; exit 2;;
esac
