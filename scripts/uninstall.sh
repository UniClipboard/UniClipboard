#!/usr/bin/env bash
#
# UniClipboard 一键卸载脚本（Linux / macOS）
#
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/UniClipboard/UniClipboard/main/scripts/uninstall.sh | bash
#   curl -fsSL .../uninstall.sh | bash -s -- --purge        # 同时清除数据目录与配置
#   curl -fsSL .../uninstall.sh | bash -s -- --dry-run      # 仅列出会删的内容
#   curl -fsSL .../uninstall.sh | bash -s -- --prefix "$HOME/Applications"
#
# 选项：
#   --purge        同时删除配置/缓存/数据目录（默认仅删应用本体）
#   --dry-run      仅列出将要删除的路径，不执行实际删除
#   --prefix DIR   覆盖默认安装目录（同 install.sh）
#   --yes          跳过 3 秒倒计时
#
# 自动探测：会同时清理 deb / rpm / AppImage 三种 Linux 安装路径，以及 macOS 的
# /Applications 和 ~/Applications 下的 UniClipboard.app。

set -euo pipefail

REPO="UniClipboard/UniClipboard"
APP_NAME="UniClipboard"
APP_BIN="uniclipboard"
APP_ID="app.uniclipboard.desktop"

PURGE=0
DRY=0
ASSUME_YES=0
PREFIX="${UC_PREFIX:-}"

usage() {
  cat <<'EOF'
UniClipboard uninstaller (Linux / macOS)

Usage:
  uninstall.sh [--purge] [--dry-run] [--yes] [--prefix DIR]

  --purge        also delete config / cache / data directories
  --dry-run      list what would be removed, do nothing
  --yes          skip the 3-second countdown
  --prefix DIR   override install dir (same semantics as install.sh)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --purge)   PURGE=1; shift ;;
    --dry-run) DRY=1; shift ;;
    --yes|-y)  ASSUME_YES=1; shift ;;
    --prefix)
      [[ $# -ge 2 ]] || { echo "缺少 --prefix 的参数" >&2; exit 1; }
      PREFIX="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "未知参数：$1" >&2; usage; exit 1 ;;
  esac
done

# ---- 输出工具 ----------------------------------------------------------------
if [[ -t 1 ]]; then
  C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_CYAN=$'\033[36m'; C_YEL=$'\033[33m'; C_RESET=$'\033[0m'
else
  C_RED=""; C_GREEN=""; C_CYAN=""; C_YEL=""; C_RESET=""
fi
info() { printf '%s==>%s %s\n' "$C_CYAN"  "$C_RESET" "$*"; }
ok()   { printf '%s✔%s %s\n'  "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%s⚠%s %s\n'  "$C_YEL"   "$C_RESET" "$*"; }
die()  { printf '%s错误:%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

# ---- 检测 OS -----------------------------------------------------------------
case "$(uname -s)" in
  Linux)  OS=linux ;;
  Darwin) OS=macos ;;
  *) die "不支持的操作系统：$(uname -s)" ;;
esac

SUDO=""
if [[ $EUID -ne 0 ]] && command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
fi

# ---- 收集要删除的路径与要执行的卸载命令 --------------------------------------
# REMOVE_PATHS: 一行一个路径，需用 rm -rf 的（前缀以 SUDO:1 标记需要 sudo）
# PM_COMMANDS:  一行一条命令，前缀同上
REMOVE_PATHS=()
PM_COMMANDS=()

add_path() {
  local p="$1" need_sudo="${2:-0}"
  # 不展开存在性，由 dry-run 输出和实际执行阶段各自判断
  REMOVE_PATHS+=("${need_sudo}|${p}")
}
add_cmd() {
  local need_sudo="$1"; shift
  PM_COMMANDS+=("${need_sudo}|$*")
}

# ---- macOS 路径 --------------------------------------------------------------
collect_macos() {
  # 应用本体：默认 /Applications + ~/Applications，加上 --prefix
  local candidates=()
  if [[ -n "$PREFIX" ]]; then candidates+=("$PREFIX"); fi
  candidates+=("/Applications" "$HOME/Applications")

  local seen=""
  for d in "${candidates[@]}"; do
    case "${seen}" in *"|${d}|"*) continue ;; esac
    seen="${seen}|${d}|"
    local app="${d}/${APP_NAME}.app"
    if [[ -d "$app" ]]; then
      if [[ -w "$d" ]]; then add_path "$app" 0
      else add_path "$app" 1
      fi
    fi
  done

  # brew 装的 cask 走 brew 卸载更稳
  if command -v brew >/dev/null 2>&1 \
     && brew list --cask uniclipboard >/dev/null 2>&1; then
    add_cmd 0 "brew uninstall --cask uniclipboard"
  fi
  if command -v brew >/dev/null 2>&1 \
     && brew list --formula uniclipboard >/dev/null 2>&1; then
    add_cmd 0 "brew uninstall --formula uniclipboard"
  fi

  if [[ "$PURGE" -eq 1 ]]; then
    # glob：默认 + 任意 profile 后缀（-dev 等）
    add_path "$HOME/Library/Application Support/${APP_ID}" 0
    for p in "$HOME/Library/Application Support/${APP_ID}-"*; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
    add_path "$HOME/Library/Caches/${APP_ID}" 0
    for p in "$HOME/Library/Caches/${APP_ID}-"*; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
    add_path "$HOME/Library/Logs/${APP_ID}" 0
    for p in "$HOME/Library/Logs/${APP_ID}-"*; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
    for p in "$HOME/Library/Preferences/${APP_ID}"*.plist; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
    for p in "$HOME/Library/Saved Application State/${APP_ID}"*.savedState; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
  fi
}

# ---- Linux 路径 --------------------------------------------------------------
collect_linux() {
  # deb
  if command -v dpkg >/dev/null 2>&1 && dpkg -s "$APP_BIN" >/dev/null 2>&1; then
    if command -v apt-get >/dev/null 2>&1; then
      add_cmd 1 "apt-get remove -y $APP_BIN"
    else
      add_cmd 1 "dpkg -r $APP_BIN"
    fi
  fi
  # rpm
  if command -v rpm >/dev/null 2>&1 && rpm -q "$APP_BIN" >/dev/null 2>&1; then
    if   command -v dnf >/dev/null 2>&1; then add_cmd 1 "dnf remove -y $APP_BIN"
    elif command -v yum >/dev/null 2>&1; then add_cmd 1 "yum remove -y $APP_BIN"
    else                                       add_cmd 1 "rpm -e $APP_BIN"
    fi
  fi

  # AppImage 安装：默认 PREFIX 是 ~/.local
  local prefixes=()
  if [[ -n "$PREFIX" ]]; then prefixes+=("$PREFIX"); fi
  prefixes+=("$HOME/.local" "/usr/local" "/usr")

  local seen=""
  for pfx in "${prefixes[@]}"; do
    case "${seen}" in *"|${pfx}|"*) continue ;; esac
    seen="${seen}|${pfx}|"
    local need_sudo=0
    [[ -w "$pfx" ]] || need_sudo=1
    local app="${pfx}/bin/${APP_NAME}.AppImage"
    local desktop="${pfx}/share/applications/${APP_ID}.desktop"
    local icon_glob="${pfx}/share/icons/hicolor"
    [[ -e "$app" ]] && add_path "$app" "$need_sudo"
    [[ -e "$desktop" ]] && add_path "$desktop" "$need_sudo"
    if [[ -d "$icon_glob" ]]; then
      while IFS= read -r icon; do
        [[ -n "$icon" ]] && add_path "$icon" "$need_sudo"
      done < <(find "$icon_glob" -type f -name "${APP_ID}.png" 2>/dev/null)
    fi
  done

  if [[ "$PURGE" -eq 1 ]]; then
    add_path "$HOME/.local/share/${APP_ID}" 0
    for p in "$HOME/.local/share/${APP_ID}-"*; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
    add_path "$HOME/.config/${APP_ID}" 0
    for p in "$HOME/.config/${APP_ID}-"*; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
    add_path "$HOME/.cache/${APP_ID}" 0
    for p in "$HOME/.cache/${APP_ID}-"*; do
      [[ -e "$p" ]] && add_path "$p" 0
    done
  fi
}

[[ "$OS" == "macos" ]] && collect_macos
[[ "$OS" == "linux" ]] && collect_linux

# ---- 去重并过滤实际存在的路径 ------------------------------------------------
filtered_paths=()
seen_paths=""
for entry in "${REMOVE_PATHS[@]:-}"; do
  [[ -z "$entry" ]] && continue
  local_path="${entry#*|}"
  [[ -e "$local_path" || -L "$local_path" ]] || continue
  case "${seen_paths}" in *"|${local_path}|"*) continue ;; esac
  seen_paths="${seen_paths}|${local_path}|"
  filtered_paths+=("$entry")
done

if [[ ${#filtered_paths[@]} -eq 0 && ${#PM_COMMANDS[@]} -eq 0 ]]; then
  warn "未发现 ${APP_NAME} 的安装痕迹。"
  if [[ "$PURGE" -ne 1 ]]; then
    info "如果只想清理数据/配置，请追加 --purge 重试。"
  fi
  exit 0
fi

# ---- 打印计划 ---------------------------------------------------------------
info "将要执行的卸载操作："
for cmd in "${PM_COMMANDS[@]:-}"; do
  [[ -z "$cmd" ]] && continue
  s="${cmd%%|*}"; c="${cmd#*|}"
  [[ "$s" == 1 ]] && c="sudo ${c}"
  printf '   - %s\n' "$c"
done
for entry in "${filtered_paths[@]}"; do
  s="${entry%%|*}"; p="${entry#*|}"
  prefix="rm -rf"
  [[ "$s" == 1 ]] && prefix="sudo ${prefix}"
  printf '   - %s %s\n' "$prefix" "$p"
done

if [[ "$DRY" -eq 1 ]]; then
  info "--dry-run，不执行删除。"
  exit 0
fi

if [[ "$ASSUME_YES" -ne 1 ]]; then
  info "3 秒后开始（Ctrl-C 中止）…"
  sleep 3
fi

# ---- 执行 -------------------------------------------------------------------
# 先尝试关闭运行中的实例（最佳努力）
if [[ "$OS" == "macos" ]]; then
  if command -v osascript >/dev/null 2>&1; then
    osascript -e "tell application \"${APP_NAME}\" to quit" >/dev/null 2>&1 || true
  fi
  pkill -f "${APP_NAME}.app/Contents/MacOS" 2>/dev/null || true
else
  pkill -x "${APP_BIN}" 2>/dev/null || true
fi

for cmd in "${PM_COMMANDS[@]:-}"; do
  [[ -z "$cmd" ]] && continue
  s="${cmd%%|*}"; c="${cmd#*|}"
  info "执行：${c}"
  if [[ "$s" == 1 ]]; then
    [[ -n "$SUDO" ]] || die "需要 sudo 但未找到 sudo 命令。"
    # shellcheck disable=SC2086
    $SUDO $c
  else
    # shellcheck disable=SC2086
    $c
  fi
done

for entry in "${filtered_paths[@]}"; do
  s="${entry%%|*}"; p="${entry#*|}"
  info "删除：${p}"
  if [[ "$s" == 1 ]]; then
    [[ -n "$SUDO" ]] || die "目录 ${p} 需要 sudo 但未找到 sudo 命令。"
    $SUDO rm -rf "$p"
  else
    rm -rf "$p"
  fi
done

# ---- 收尾：刷新桌面数据库 ----------------------------------------------------
if [[ "$OS" == "linux" ]] && command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
fi

ok "${APP_NAME} 卸载完成。"
if [[ "$PURGE" -ne 1 ]]; then
  info "数据/配置目录已保留。如要彻底清除，重跑并加上 --purge。"
fi
