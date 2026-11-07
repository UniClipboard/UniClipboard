# 0001 · 添加设备：6 位配对码 + 完整地址

- 状态：已采纳
- 日期：2026-11-07
- 原型：[`prototype/gui.html`](../../../prototype/gui.html)（场景「添加设备」「配对码服务不可用」）

## 背景

原先只能用 `unic addr` 输出的完整地址配对。完整地址很长，内含 WireGuard 预共享密钥，跨设备手抄或转发都不方便。
uc-website 项目中的 try.uniclipboard.app 已有一套成熟的 6 位码机制（`src/lib/web-transfer/short-code.ts`），
背后是 uc-rendezvous 的 `/v1/web-pairings` 路由。

## 决策

1. **配对码复用 rendezvous 的 `/v1/web-pairings`**（create / resolve / consume），不另开路由。
   - `POST /v1/web-pairings {ticket}` → `{code:"NNN-NNN", expiresAtMs}`
   - `POST /v1/web-pairings/resolve {code}` → `{ticket, expiresAtMs}`（在 consume 或过期前可重复）
   - `POST /v1/web-pairings/consume {code}` → `{ok:true}`
   - 错误：400 无效，404 未知或过期，409 已使用，429 限速，5xx 故障。
   - 由 daemon 直接调用（不经浏览器，不受 CORS 限制）。地址可用 `UNIC_RENDEZVOUS_URL` 覆盖，默认 `https://rendezvous.uniclipboard.app`。
2. **TTL 与限速沿用网页端**：5 分钟有效、仅可使用一次；服务端限速（create 10/分钟、resolve 20/分钟、consume 20/分钟，按 IP）。
   本地截止时间按 `localDeadline` 的规则换算并钳制（不超过发出请求后 300 秒，不少于 5 秒），不信任服务器时钟。
3. **票据使用独立的 kind**：`{"v":1,"kind":"unic-pair","c":<地址>,"k":<token>}`。
   网页端只接受 `uc-web-try`，unic 只接受 `unic-pair`，两边的码互不解析。`k` 是每个码新生成的 128 位随机 token。
4. **配对码只负责“找到地址”，不授予信任**。
   - 入码方拿到地址后照常拨号，在 hello 里附上 token；出码方用 token 确认对方确实是凭当前有效的码来的，
     随即让这个码失效（本地作废 + consume），把对方记为「通过配对码」的待批准设备。
   - 出码方的用户**必须批准**，批准前双方都显示同一个指纹供核对。
   - 理由：6 位只有 100 万种，且 resolve 在 consume 前可重复。猜中码的人能拿到完整地址，但仍然连不进来。
5. **指纹**：`SHA-256("unic-pair-fp\x00" + 较小的设备 ID + "\x00" + 较大的设备 ID)` 取前 8 字节，显示为 4 组 4 位大写十六进制。
   双方各自独立计算。攻击者不知道真正入码方的设备 ID，无法让自己的指纹与之相同。
6. **完整地址保留**，作为兜底：配对码服务不可用、命令行、或不想经过第三方服务时使用。界面上默认折叠并打码，常驻「地址等同密码」提示。
7. **仅桌面端**。暂不做手机 App，也不做二维码和深链接。

## 流程

```
出码方 A                         rendezvous                     入码方 B
POST /v1/code ──create(ticket)──▶ 482-913
显示 482 913                                     ◀──resolve── 用户输入 482913
                                                ──ticket──▶ 解析出 A 的地址与 token
◀──────────────── 拨号，hello{…, pair:token} ─────────────────
token 匹配：作废本码、consume                                   B 视 A 为已批准（主动配对）
B 记为「通过配对码」待批准，显示指纹 ◀── verdict ok=false ──▶ B 显示「等待批准」+ 同一指纹
用户核对并批准 ── 拨号 B ───────────────────────────────────▶ 双方连接、开始同步
```

## 不做的事 / 以后再议

- 二维码、手机 App、`unic://` 深链接。
- 配对码的自动续期只在 GUI 停留在「让设备加入本机」时进行，避免无谓地占用 create 限额。
