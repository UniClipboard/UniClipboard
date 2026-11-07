# unic

跨设备剪贴板同步。Go 实现:网络层用 [tailcat](https://github.com/tailscale/tailcat)(点对点 WireGuard 隧道,无账号、无控制服务器),界面用 [mygo](https://github.com/egoist/mygo)(原生 UI,无 webview),存储用 SQLite(纯 Go,无 cgo)。

## 架构

```
 unic cli ─┐                       ┌─ 设备 B 的 daemon
          ├─ unix socket (HTTP) ─ daemon ── tailcat 隧道 ──┤
 unic gui ─┘   像 docker 一样       │  (mesh,直连优先,DERP 兜底)
                                  └─ 设备 C 的 daemon
```

- `unicd`:常驻节点,是 mygo 的无窗口应用(剪贴板必须在主线程访问),带托盘/菜单栏图标。监听剪贴板、与所有已配对设备保持连接、应用远端变化。
- `unic gui` / `unic <cmd>`:都只是 daemon 的客户端,通过 `UNIC_HOME/unic.sock` 说 HTTP+JSON(`curl --unix-socket` 也能用)。
- 配对:`unic addr` 得到本机地址,在另一台设备上 `unic pair <地址>`。配对是双向的,之后重启会自动重连(mesh,每台都互连)。
- **地址等同密码**:它内含 WireGuard 预共享密钥,持有者即可连接并写入你的剪贴板。

## 同步内容

文本、富文本(HTML)、图片(PNG)、文件(普通文件,单个上限 2 GiB)。启动时已在剪贴板里的内容不会被广播。

## 目录

| 路径 | 作用 |
|---|---|
| `internal/engine` | 同步核心:轮询剪贴板、会话管理、防回环、去重 |
| `internal/wire` | 设备间线协议与大小限制 |
| `internal/tcnet` | tailcat 传输层(密钥与地址持久化) |
| `internal/board` | mygo 剪贴板适配 |
| `internal/api` | 本地 socket API 与客户端 |
| `internal/store` | SQLite:身份、已配对设备 |
| `cmd/unicd` | daemon(托盘图标) |
| `cmd/unic` | cli 与 gui |

## 开发

```sh
go build -o unic ./cmd/unic && go build -o unicd ./cmd/unicd
go test -race ./...
UNIC_HOME=/tmp/unicA ./unicd        # 用 UNIC_HOME 在同一台机器上跑第二个节点
```

国内网络下载依赖慢时:`GOPROXY=https://goproxy.cn,direct go mod download`。

## 第一阶段范围外

历史记录与搜索、文件夹同步、选择性接收/设备授权确认、原生剪贴板变化通知(目前 400ms 轮询)。
