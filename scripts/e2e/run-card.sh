#!/usr/bin/env bash
# 本地 (mac) 触发：把 working tree 同步到远端 Linux 节点，启动 wdio 跑指定卡片对应的 spec
#
# 用法：
#   scripts/e2e/run-card.sh <card-id>
#   scripts/e2e/run-card.sh --list
#
# 环境变量：
#   E2E_REMOTE_HOST   远端主机别名 (默认 fedora，需要在 ~/.ssh/config 配好)
#   E2E_REMOTE_PATH   远端项目路径 (默认 projects/uniclipboard，相对 $HOME)
#   E2E_SKIP_SYNC=1   跳过 rsync，直接用远端已有代码
#   E2E_KEEP_PROFILE=1 透传给 wdio.conf.mjs，跑完不清 profile data

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT" || { echo "无法切到 REPO_ROOT: $REPO_ROOT" >&2; exit 1; }

REMOTE_HOST="${E2E_REMOTE_HOST:-fedora}"
REMOTE_PATH="${E2E_REMOTE_PATH:-projects/uniclipboard}"

list_cards() {
  for f in e2e/cards/*.md; do
    name=$(basename "$f" .md)
    case "$name" in
      SCHEMA|README) continue ;;
    esac
    echo "$name"
  done
}

if [ "${1:-}" = "--list" ] || [ "${1:-}" = "" ]; then
  echo "用法: $0 <card-id>"
  echo
  echo "可用卡片："
  list_cards | sed 's/^/  /'
  exit 2
fi

CARD_ID="$1"
SPEC_PATH="e2e/specs/${CARD_ID}.e2e.js"

if [ ! -f "e2e/cards/${CARD_ID}.md" ]; then
  echo "卡片不存在: e2e/cards/${CARD_ID}.md" >&2
  exit 2
fi

if [ ! -f "$SPEC_PATH" ]; then
  echo "Spec 不存在: $SPEC_PATH" >&2
  echo "为卡片 $CARD_ID 编写 spec 后再运行" >&2
  exit 2
fi

# 1) 同步代码（保留远端 .git；node_modules 与构建产物不传）
if [ "${E2E_SKIP_SYNC:-0}" != "1" ]; then
  echo "[1/3] rsync → $REMOTE_HOST:~/$REMOTE_PATH"
  rsync -a --delete \
    --exclude='.git' \
    --exclude='node_modules' \
    --exclude='src-tauri/target' \
    --exclude='dist' \
    --exclude='.vite' \
    --exclude='.idea' \
    --exclude='.vscode' \
    --exclude='*.log' \
    --exclude='.planning' \
    --exclude='docs-site/node_modules' \
    ./ "$REMOTE_HOST:$REMOTE_PATH/"
else
  echo "[1/3] 跳过 rsync (E2E_SKIP_SYNC=1)"
fi

# 2a) 清理上次留下的 wdio 残留: 按端口找 PID 比按进程名安全
# 注意不要用 `pgrep -f` —— ssh 这条命令的 cmdline 也会被 match
echo "[2/3a] 清理远端 wdio 残留进程 (按 4444/4445 占用)"
ssh "$REMOTE_HOST" '
  for port in 4444 4445; do
    for pid in $(lsof -ti :$port 2>/dev/null); do
      kill -9 $pid 2>/dev/null
    done
  done
  true
'

# 2b) 在远端 install + 跑 wdio 单 spec
echo "[2/3b] 远端运行 wdio: $SPEC_PATH"
ssh "$REMOTE_HOST" "cd ~/$REMOTE_PATH && \
  export PATH=\$HOME/.bun/bin:\$HOME/.cargo/bin:\$PATH && \
  export E2E_KEEP_PROFILE='${E2E_KEEP_PROFILE:-0}' && \
  bun install --ignore-scripts >/dev/null 2>&1 && \
  bunx wdio run e2e/wdio.conf.mjs --spec $SPEC_PATH"
EXIT_CODE=$?

# 3) 抓远端日志摘要（最多 3 个最近 log 文件名，给归因 agent 起点）
echo "[3/3] 远端日志摘要"
ssh "$REMOTE_HOST" "ls -t ~/.local/share/app.uniclipboard.desktop-wdio/logs 2>/dev/null | head -3" || true

exit $EXIT_CODE
