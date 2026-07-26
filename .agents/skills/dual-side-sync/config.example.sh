# sync-to-win configuration template.
#
# Copy this file to `config.local.sh` (gitignored) and fill in the values for
# your Windows peer. The script auto-sources `config.local.sh` from the same
# directory on every run; nothing else needs to be wired up.
#
#   cp .agents/skills/dual-side-sync/config.example.sh \
#      .agents/skills/dual-side-sync/config.local.sh
#   $EDITOR .agents/skills/dual-side-sync/config.local.sh

# --- required ----------------------------------------------------------------

# IPv4/IPv6 address or hostname of the Windows machine (the SSH server).
WIN_HOST="192.168.1.50"

# Username on the Windows side. Match the login you use to RDP / SSH today.
WIN_USER="mark"

# Absolute path of the repo on the Windows machine, in a form that the remote
# shell understands. Two common formats:
#   * Git-Bash / MSYS2 / Cygwin / WSL bash:   /c/Users/mark/projects/UniClipboard
#   * Native PowerShell / cmd:                C:/Users/mark/projects/UniClipboard
# Forward slashes are safest. Avoid spaces in the path.
WIN_REPO="/c/Users/mark/projects/UniClipboard"

# --- authentication (pick exactly one) --------------------------------------

# (a) Password auth — convenient but requires `sshpass` on this Mac.
#       brew install hudochenkov/sshpass/sshpass
#     Leave empty to disable password auth.
WIN_PASS=""

# (b) SSH key auth — leave WIN_PASS empty and either:
#       * rely on ssh-agent / your ~/.ssh/config, or
#       * point WIN_KEY at a specific private key file.
WIN_KEY=""

# --- optional ----------------------------------------------------------------

# SSH port on the Windows machine. OpenSSH for Windows defaults to 22.
WIN_PORT="22"

# Local repo (defaults to the current project root). Override only if you run
# the skill from a different working tree.
# MAC_REPO="/Volumes/ExternalSSD/superset/uniclipboard/slender-soybean"

# Extra paths to *exclude* from rsync, in addition to .git/, target/,
# node_modules/, and dist/ which are always excluded. One pattern per element.
# EXTRA_EXCLUDES=(".env.local" "*.log")
