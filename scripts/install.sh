#!/usr/bin/env bash
#
# UniClipboard one-shot installer (Linux / macOS)
#
# Usage (pick one):
#   curl -fsSL https://raw.githubusercontent.com/UniClipboard/UniClipboard/main/scripts/install.sh | bash
#   curl -fsSL .../install.sh | bash -s -- --version v0.9.0
#   curl -fsSL .../install.sh | bash -s -- --format appimage
#   curl -fsSL .../install.sh | bash -s -- --prefix "$HOME/Applications"   # macOS, user-level
#
# Environment variables:
#   UC_VERSION      Specific version (e.g. v0.9.0); defaults to latest
#   UC_FORMAT       Force install format: deb | rpm | snap | appimage | app
#   UC_PREFIX       Install directory override:
#                     - macOS (app)      defaults to /Applications
#                     - Linux (appimage) defaults to $HOME/.local
#   UC_REPO         GitHub repo, defaults to UniClipboard/UniClipboard
#   GITHUB_TOKEN    Optional, sidesteps the GitHub API rate limit
#
# Auto-detection:
#   - macOS:        download .app.tar.gz and move it to PREFIX
#   - Ubuntu 20.04: snap (host lacks libwebkit2gtk-4.1, .deb won't launch)
#   - Other Linux:  with root/sudo + apt/dpkg → deb; dnf/yum/rpm → rpm; otherwise AppImage
#
# Note: on macOS you can also use Homebrew — brew install --cask uniclipboard

set -euo pipefail

REPO="${UC_REPO:-UniClipboard/UniClipboard}"
APP_NAME="UniClipboard"
APP_BIN="uniclipboard"
APP_ID="app.uniclipboard.desktop"

VERSION="${UC_VERSION:-}"
FORMAT="${UC_FORMAT:-}"
PREFIX="${UC_PREFIX:-}"

usage() {
  cat <<'EOF'
UniClipboard installer (Linux / macOS)

Usage:
  install.sh [--version vX.Y.Z] [--format deb|rpm|snap|appimage|app] [--prefix DIR]

Environment:
  UC_VERSION, UC_FORMAT, UC_PREFIX, UC_REPO, GITHUB_TOKEN

Examples:
  install.sh
  install.sh --version v0.9.0
  install.sh --format snap                  # recommended on Ubuntu 20.04
  install.sh --format appimage              # Linux, no sudo
  install.sh --prefix "$HOME/Applications"  # macOS, user-level
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) [[ $# -ge 2 ]] || { echo "missing argument for --version" >&2; exit 1; }; VERSION="$2"; shift 2 ;;
    --format)  [[ $# -ge 2 ]] || { echo "missing argument for --format"  >&2; exit 1; }; FORMAT="$2";  shift 2 ;;
    --prefix)  [[ $# -ge 2 ]] || { echo "missing argument for --prefix"  >&2; exit 1; }; PREFIX="$2";  shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 1 ;;
  esac
done

# ---- Output helpers ---------------------------------------------------------
if [[ -t 1 ]]; then
  C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_CYAN=$'\033[36m'; C_RESET=$'\033[0m'
else
  C_RED=""; C_GREEN=""; C_CYAN=""; C_RESET=""
fi
info() { printf '%s==>%s %s\n' "$C_CYAN" "$C_RESET" "$*"; }
ok()   { printf '%s✔%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
die()  { printf '%sError:%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

# ---- Detect OS / arch -------------------------------------------------------
OS_KERNEL="$(uname -s)"
case "$OS_KERNEL" in
  Linux)  OS=linux ;;
  Darwin) OS=macos ;;
  *) die "unsupported OS: ${OS_KERNEL} (only Linux and macOS are supported)" ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)
    DEB_ARCH=amd64; RPM_ARCH=x86_64; AI_ARCH=amd64
    MAC_ARCH=x86_64-apple-darwin
    ;;
  aarch64|arm64)
    DEB_ARCH=arm64; RPM_ARCH=aarch64; AI_ARCH=aarch64
    MAC_ARCH=aarch64-apple-darwin
    ;;
  *) die "unsupported architecture: $ARCH" ;;
esac

# ---- Dependencies / sudo ----------------------------------------------------
command -v curl >/dev/null 2>&1 || die "curl is required; please install it first."
command -v tar  >/dev/null 2>&1 || die "tar is required; please install it first."

SUDO=""
if [[ $EUID -ne 0 ]] && command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
fi

# ---- Detect distro / version ------------------------------------------------
DISTRO_ID=""
DISTRO_VERSION_ID=""
if [[ "$OS" == "linux" && -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  DISTRO_ID="${ID:-}"
  DISTRO_VERSION_ID="${VERSION_ID:-}"
fi

is_ubuntu_2004() {
  [[ "$DISTRO_ID" == "ubuntu" && "$DISTRO_VERSION_ID" == "20.04" ]]
}

# ---- Resolve version --------------------------------------------------------
api_get() {
  local url="$1"
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    curl -fsSL -H "Authorization: Bearer ${GITHUB_TOKEN}" "$url"
  else
    curl -fsSL "$url"
  fi
}

if [[ -z "$VERSION" ]]; then
  info "Querying latest release…"
  # /releases/latest automatically skips prereleases (alpha/beta)
  VERSION="$(api_get "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | head -n1 | cut -d'"' -f4 || true)"
  [[ -n "$VERSION" ]] || die "Could not fetch the latest release tag. Set UC_VERSION or GITHUB_TOKEN and retry."
fi
[[ "$VERSION" == v* ]] || VERSION="v$VERSION"
VER_NUM="${VERSION#v}"

# ---- Pick install format ----------------------------------------------------
choose_format() {
  if [[ -n "$FORMAT" ]]; then echo "$FORMAT"; return; fi
  if [[ "$OS" == "macos" ]]; then echo app; return; fi
  # Ubuntu 20.04 hosts only ship libwebkit2gtk-4.0 (4.1 lands in 22.04), so
  # the .deb installs but won't launch. snap pulls in the gnome-42-2204
  # extension which bundles its own webkit2gtk-4.1 stack, so it just works
  # — prefer it here.
  if is_ubuntu_2004; then echo snap; return; fi
  if [[ -n "$SUDO" || $EUID -eq 0 ]]; then
    if command -v dpkg >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then echo deb; return; fi
    if command -v rpm >/dev/null 2>&1 \
       && { command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1; }; then echo rpm; return; fi
  fi
  echo appimage
}
FORMAT="$(choose_format)"

# OS and FORMAT must match
case "$OS:$FORMAT" in
  macos:app) ;;
  linux:deb|linux:rpm|linux:snap|linux:appimage) ;;
  *) die "${OS} does not support --format ${FORMAT} (macOS uses app; Linux uses deb/rpm/snap/appimage)" ;;
esac

# ---- PREFIX defaults (per format) -------------------------------------------
if [[ -z "$PREFIX" ]]; then
  case "$FORMAT" in
    app)      PREFIX="/Applications" ;;
    appimage) PREFIX="$HOME/.local" ;;
  esac
fi

info "Target version: ${VERSION}    OS: ${OS}/${ARCH}    Format: ${FORMAT}"

REL_BASE="https://github.com/${REPO}/releases/download/${VERSION}"
TMP="$(mktemp -d 2>/dev/null || mktemp -d -t 'uc-install')"
trap 'rm -rf "$TMP"' EXIT

download() {
  local url="$1" dst="$2"
  info "Downloading ${url##*/}"
  curl -fL --progress-bar -o "$dst" "$url" || die "download failed: $url"
}

# ---- macOS: .app.tar.gz → /Applications --------------------------------------
install_macos_app() {
  local file="${APP_NAME}_${MAC_ARCH}.app.tar.gz"
  download "${REL_BASE}/${file}" "${TMP}/${file}"

  info "Extracting ${APP_NAME}.app…"
  tar -xzf "${TMP}/${file}" -C "${TMP}"
  local app_src="${TMP}/${APP_NAME}.app"
  [[ -d "$app_src" ]] || die "${APP_NAME}.app not found inside the archive after extraction"

  local dst_dir="$PREFIX"
  local dst_app="${dst_dir}/${APP_NAME}.app"
  [[ -d "$dst_dir" ]] || mkdir -p "$dst_dir" 2>/dev/null || true

  # Only escalate via sudo when the target directory is not writable;
  # a normal admin account can write /Applications directly.
  local SH=""
  if [[ ! -w "$dst_dir" ]]; then
    [[ -n "$SUDO" ]] || die "${dst_dir} is not writable and sudo was not found; pass --prefix \"\$HOME/Applications\" instead."
    SH="$SUDO"
  fi

  if [[ -e "$dst_app" ]]; then
    info "Removing existing ${dst_app}"
    $SH rm -rf "$dst_app"
  fi

  info "Installing to ${dst_app}"
  $SH mv "$app_src" "$dst_app"

  # Strip the quarantine xattr inherited from curl/Firefox so Gatekeeper
  # doesn't block launch (the app itself is still signed + notarized).
  if command -v xattr >/dev/null 2>&1; then
    $SH xattr -dr com.apple.quarantine "$dst_app" 2>/dev/null || true
  fi
}

# ---- Linux: .deb -------------------------------------------------------------
install_deb() {
  local file="${APP_NAME}_${VER_NUM}_${DEB_ARCH}.deb"
  download "${REL_BASE}/${file}" "${TMP}/${file}"
  info "Installing .deb package (sudo required)…"
  if command -v apt-get >/dev/null 2>&1; then
    if ! $SUDO apt-get install -y "${TMP}/${file}"; then
      info "apt-get failed; falling back to dpkg + apt-get -f install"
      $SUDO dpkg -i "${TMP}/${file}" || true
      $SUDO apt-get install -f -y
    fi
  else
    $SUDO dpkg -i "${TMP}/${file}"
  fi
}

# ---- Linux: snap (recommended on Ubuntu 20.04 and other older distros) ------
# The snap binary loads webkit2gtk-4.1 + the full GTK stack from the
# gnome-42-2204 extension, fully decoupled from host glibc / GTK, so it runs
# on Ubuntu 20.04 (webkit2gtk-4.0 era) too. snap manages its own channel and
# update flow, so this script doesn't pass --version through.
install_snap() {
  if ! command -v snap >/dev/null 2>&1; then
    info "snapd not detected; installing it (sudo required)…"
    if [[ -z "$SUDO" && $EUID -ne 0 ]]; then
      die "root/sudo is required to install snapd; or use --format appimage."
    fi
    if command -v apt-get >/dev/null 2>&1; then
      $SUDO apt-get update -y
      $SUDO apt-get install -y snapd
    else
      die "no apt-get on this system; cannot auto-install snapd. Install it manually and retry."
    fi
    # snapd.socket must be up before `snap install` works; seeded.service
    # waits for the seed step to finish.
    if command -v systemctl >/dev/null 2>&1; then
      $SUDO systemctl enable --now snapd.socket >/dev/null 2>&1 || true
      $SUDO systemctl start snapd.seeded.service >/dev/null 2>&1 || true
    fi
  fi

  info "Installing ${APP_BIN} via snap (stable channel, sudo required)…"
  if [[ -z "$SUDO" && $EUID -ne 0 ]]; then
    die "snap install requires root/sudo."
  fi
  $SUDO snap install "$APP_BIN"

  # The password-manager-service plug is not auto-connect on the snap store
  # yet, so it must be wired up by hand after the first install — otherwise
  # AppArmor blocks the daemon's secret-service D-Bus call during startup.
  info "Next: connect the keyring plug so the daemon can reach the system keyring:"
  printf '       sudo snap connect %s:password-manager-service\n' "$APP_BIN"
}

# ---- Linux: .rpm -------------------------------------------------------------
install_rpm() {
  local file="${APP_NAME}-${VER_NUM}-1.${RPM_ARCH}.rpm"
  download "${REL_BASE}/${file}" "${TMP}/${file}"
  info "Installing .rpm package (sudo required)…"
  if command -v dnf >/dev/null 2>&1; then
    $SUDO dnf install -y "${TMP}/${file}"
  elif command -v yum >/dev/null 2>&1; then
    $SUDO yum install -y "${TMP}/${file}"
  else
    $SUDO rpm -Uvh --force "${TMP}/${file}"
  fi
}

# ---- Linux: AppImage ---------------------------------------------------------
install_appimage() {
  local file="${APP_NAME}_${VER_NUM}_${AI_ARCH}.AppImage"
  local bin_dir="${PREFIX}/bin"
  local apps_dir="${PREFIX}/share/applications"
  local icons_dir="${PREFIX}/share/icons/hicolor/512x512/apps"
  local dst="${bin_dir}/${APP_NAME}.AppImage"

  mkdir -p "$bin_dir" "$apps_dir" "$icons_dir"
  download "${REL_BASE}/${file}" "$dst"
  chmod +x "$dst"

  # Best-effort icon extraction; failure does not block install.
  (
    cd "$TMP"
    "$dst" --appimage-extract '*.png' >/dev/null 2>&1 || true
    icon_src="$(find "$TMP/squashfs-root" -maxdepth 3 -type f -name '*.png' 2>/dev/null | sort -r | head -n1 || true)"
    if [[ -n "$icon_src" ]]; then
      cp "$icon_src" "${icons_dir}/${APP_ID}.png" || true
    fi
  ) || true

  cat > "${apps_dir}/${APP_ID}.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=${APP_NAME}
Comment=Cross-device clipboard sync
Exec=${dst} %U
Icon=${APP_ID}
Terminal=false
Categories=Utility;
StartupWMClass=${APP_NAME}
EOF
  chmod 644 "${apps_dir}/${APP_ID}.desktop"

  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
  fi

  case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *)
      info "Note: ${bin_dir} is not on PATH. To run from the shell, append it:"
      printf '       echo '\''export PATH="%s:$PATH"'\'' >> ~/.profile\n' "$bin_dir"
      ;;
  esac
}

case "$FORMAT" in
  app)      install_macos_app ;;
  deb)      install_deb ;;
  rpm)      install_rpm ;;
  snap)     install_snap ;;
  appimage) install_appimage ;;
  *) die "unknown format: ${FORMAT}" ;;
esac

# snap versions are managed by the snap store, not the GitHub release tag.
if [[ "$FORMAT" == "snap" ]]; then
  ok "${APP_NAME} installed (from snap stable channel)."
else
  ok "${APP_NAME} ${VERSION} installed."
fi
case "$FORMAT" in
  app)
    info "Run: search for UniClipboard in Launchpad/Spotlight, or 'open -a UniClipboard'"
    ;;
  appimage)
    info "Run: ${PREFIX}/bin/${APP_NAME}.AppImage    or search UniClipboard in your app menu"
    ;;
  snap)
    info "Run: search for UniClipboard in your app menu, or 'snap run ${APP_BIN}'"
    ;;
  *)
    info "Run: search for UniClipboard in your app menu, or the '${APP_BIN}' command"
    ;;
esac
