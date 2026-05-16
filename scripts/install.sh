#!/usr/bin/env bash
#
# UniClipboard 一键安装脚本（Linux / macOS）
#
# 用法（任选其一）：
#   curl -fsSL https://raw.githubusercontent.com/UniClipboard/UniClipboard/main/scripts/install.sh | bash
#   curl -fsSL .../install.sh | bash -s -- --version v0.9.0
#   curl -fsSL .../install.sh | bash -s -- --format appimage
#   curl -fsSL .../install.sh | bash -s -- --prefix "$HOME/Applications"   # macOS 用户级
#
# 环境变量：
#   UC_VERSION      指定版本（如 v0.9.0），默认 latest
#   UC_FORMAT       强制安装格式：deb | rpm | appimage | app
#   UC_PREFIX       安装目录覆盖：
#                     - macOS（app）默认 /Applications
#                     - Linux（appimage）默认 $HOME/.local
#   UC_REPO         GitHub 仓库，默认 UniClipboard/UniClipboard
#   GITHUB_TOKEN    可选，规避 GitHub API 速率限制
#
# 自动检测：
#   - macOS:  下载 .app.tar.gz 并搬到 PREFIX
#   - Linux:  有 root/sudo + apt/dpkg → deb；dnf/yum/rpm → rpm；否则 AppImage
#
# 提示：macOS 也可使用 Homebrew —— brew install --cask uniclipboard

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
  install.sh [--version vX.Y.Z] [--format deb|rpm|appimage|app] [--prefix DIR]

Environment:
  UC_VERSION, UC_FORMAT, UC_PREFIX, UC_REPO, GITHUB_TOKEN

Examples:
  install.sh
  install.sh --version v0.9.0
  install.sh --format appimage              # Linux, no sudo
  install.sh --prefix "$HOME/Applications"  # macOS, user-level
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) [[ $# -ge 2 ]] || { echo "缺少 --version 的参数" >&2; exit 1; }; VERSION="$2"; shift 2 ;;
    --format)  [[ $# -ge 2 ]] || { echo "缺少 --format 的参数"  >&2; exit 1; }; FORMAT="$2";  shift 2 ;;
    --prefix)  [[ $# -ge 2 ]] || { echo "缺少 --prefix 的参数"  >&2; exit 1; }; PREFIX="$2";  shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "未知参数：$1" >&2; usage; exit 1 ;;
  esac
done

# ---- 输出工具 ----------------------------------------------------------------
if [[ -t 1 ]]; then
  C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_CYAN=$'\033[36m'; C_RESET=$'\033[0m'
else
  C_RED=""; C_GREEN=""; C_CYAN=""; C_RESET=""
fi
info() { printf '%s==>%s %s\n' "$C_CYAN" "$C_RESET" "$*"; }
ok()   { printf '%s✔%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
die()  { printf '%s错误:%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

# ---- 检测 OS / 架构 ----------------------------------------------------------
OS_KERNEL="$(uname -s)"
case "$OS_KERNEL" in
  Linux)  OS=linux ;;
  Darwin) OS=macos ;;
  *) die "不支持的操作系统：${OS_KERNEL}（仅支持 Linux 和 macOS）" ;;
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
  *) die "不支持的架构：$ARCH" ;;
esac

# ---- 依赖 / sudo --------------------------------------------------------------
command -v curl >/dev/null 2>&1 || die "缺少 curl，请先安装。"
command -v tar  >/dev/null 2>&1 || die "缺少 tar，请先安装。"

SUDO=""
if [[ $EUID -ne 0 ]] && command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
fi

# ---- 解析版本号 --------------------------------------------------------------
api_get() {
  local url="$1"
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    curl -fsSL -H "Authorization: Bearer ${GITHUB_TOKEN}" "$url"
  else
    curl -fsSL "$url"
  fi
}

if [[ -z "$VERSION" ]]; then
  info "查询最新发布版本…"
  # /releases/latest 自动跳过 prerelease（alpha/beta）
  VERSION="$(api_get "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | head -n1 | cut -d'"' -f4 || true)"
  [[ -n "$VERSION" ]] || die "无法获取 latest release tag。可设置 UC_VERSION 或 GITHUB_TOKEN 后重试。"
fi
[[ "$VERSION" == v* ]] || VERSION="v$VERSION"
VER_NUM="${VERSION#v}"

# ---- 选择安装格式 -------------------------------------------------------------
choose_format() {
  if [[ -n "$FORMAT" ]]; then echo "$FORMAT"; return; fi
  if [[ "$OS" == "macos" ]]; then echo app; return; fi
  if [[ -n "$SUDO" || $EUID -eq 0 ]]; then
    if command -v dpkg >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then echo deb; return; fi
    if command -v rpm >/dev/null 2>&1 \
       && { command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1; }; then echo rpm; return; fi
  fi
  echo appimage
}
FORMAT="$(choose_format)"

# OS 与 FORMAT 必须匹配
case "$OS:$FORMAT" in
  macos:app) ;;
  linux:deb|linux:rpm|linux:appimage) ;;
  *) die "${OS} 上不支持 --format ${FORMAT}（macOS 用 app；Linux 用 deb/rpm/appimage）" ;;
esac

# ---- PREFIX 默认值（按格式区分） ----------------------------------------------
if [[ -z "$PREFIX" ]]; then
  case "$FORMAT" in
    app)      PREFIX="/Applications" ;;
    appimage) PREFIX="$HOME/.local" ;;
  esac
fi

info "目标版本：${VERSION}    系统：${OS}/${ARCH}    安装方式：${FORMAT}"

REL_BASE="https://github.com/${REPO}/releases/download/${VERSION}"
TMP="$(mktemp -d 2>/dev/null || mktemp -d -t 'uc-install')"
trap 'rm -rf "$TMP"' EXIT

download() {
  local url="$1" dst="$2"
  info "下载 ${url##*/}"
  curl -fL --progress-bar -o "$dst" "$url" || die "下载失败：$url"
}

# ---- macOS: .app.tar.gz → /Applications --------------------------------------
install_macos_app() {
  local file="${APP_NAME}_${MAC_ARCH}.app.tar.gz"
  download "${REL_BASE}/${file}" "${TMP}/${file}"

  info "解压 ${APP_NAME}.app…"
  tar -xzf "${TMP}/${file}" -C "${TMP}"
  local app_src="${TMP}/${APP_NAME}.app"
  [[ -d "$app_src" ]] || die "解压后未在归档中找到 ${APP_NAME}.app"

  local dst_dir="$PREFIX"
  local dst_app="${dst_dir}/${APP_NAME}.app"
  [[ -d "$dst_dir" ]] || mkdir -p "$dst_dir" 2>/dev/null || true

  # 仅当目标目录不可写时再调用 sudo；普通管理员账户对 /Applications 直接可写
  local SH=""
  if [[ ! -w "$dst_dir" ]]; then
    [[ -n "$SUDO" ]] || die "目录 ${dst_dir} 不可写且未找到 sudo；改用 --prefix \"\$HOME/Applications\"。"
    SH="$SUDO"
  fi

  if [[ -e "$dst_app" ]]; then
    info "移除已有的 ${dst_app}"
    $SH rm -rf "$dst_app"
  fi

  info "安装到 ${dst_app}"
  $SH mv "$app_src" "$dst_app"

  # 清除从浏览器/curl 继承下来的隔离属性，避免 Gatekeeper 误拦
  # （app 自身仍由签名 + 公证把关）
  if command -v xattr >/dev/null 2>&1; then
    $SH xattr -dr com.apple.quarantine "$dst_app" 2>/dev/null || true
  fi
}

# ---- Linux: .deb -------------------------------------------------------------
install_deb() {
  local file="${APP_NAME}_${VER_NUM}_${DEB_ARCH}.deb"
  download "${REL_BASE}/${file}" "${TMP}/${file}"
  info "安装 .deb 包（需要 sudo）…"
  if command -v apt-get >/dev/null 2>&1; then
    if ! $SUDO apt-get install -y "${TMP}/${file}"; then
      info "apt-get 失败，回退到 dpkg + apt-get -f install"
      $SUDO dpkg -i "${TMP}/${file}" || true
      $SUDO apt-get install -f -y
    fi
  else
    $SUDO dpkg -i "${TMP}/${file}"
  fi
}

# ---- Linux: .rpm -------------------------------------------------------------
install_rpm() {
  local file="${APP_NAME}-${VER_NUM}-1.${RPM_ARCH}.rpm"
  download "${REL_BASE}/${file}" "${TMP}/${file}"
  info "安装 .rpm 包（需要 sudo）…"
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

  # 尽力提取图标，失败不影响安装
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
      info "提示：${bin_dir} 不在 PATH 中。若想在终端直接运行命令，可执行："
      printf '       echo '\''export PATH="%s:$PATH"'\'' >> ~/.profile\n' "$bin_dir"
      ;;
  esac
}

case "$FORMAT" in
  app)      install_macos_app ;;
  deb)      install_deb ;;
  rpm)      install_rpm ;;
  appimage) install_appimage ;;
  *) die "未知格式：${FORMAT}" ;;
esac

ok "${APP_NAME} ${VERSION} 安装完成。"
case "$FORMAT" in
  app)
    info "运行：在 Launchpad/Spotlight 搜索 UniClipboard，或执行 'open -a UniClipboard'"
    ;;
  appimage)
    info "运行：${PREFIX}/bin/${APP_NAME}.AppImage    或在应用菜单搜索 UniClipboard"
    ;;
  *)
    info "运行：在应用菜单搜索 UniClipboard，或执行 ${APP_BIN}"
    ;;
esac
