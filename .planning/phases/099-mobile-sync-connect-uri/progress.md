# Progress: 099 · 移动端扫码接入协议

## 阶段总览

| 阶段 | 状态 | Commit / PR | 备注 |
|---|---|---|---|
| 阶段 0: 协议规范文档 | ✅ 完成 | `ec59277b` | `docs/architecture/mobile-sync-connect-uri.md`, 含 §7 golden vector |
| 阶段 1: Rust 编解码 + 22 测试 | ✅ 完成 | `ec59277b` | `connect_uri.rs`, 与规范字节一致 |
| 阶段 2: Rust use case + DTO | ✅ 完成 | `3756c84e` | `register_device.rs` 改 QR 内容; `mobile_sync.rs` DTO 加 `connectUri`; bindings 自动重生; 顺手清掉 phase 1 留下的 10 个 dead-code 警告 |
| 阶段 3A: TS 解析器 + Vitest | ✅ 完成 | (本地未提交) | `src/lib/mobileSyncConnectUri.ts` + 跨语言 golden vector 22 测试 |
| 阶段 3B: 凭据弹窗 UI | ✅ 完成 | (本地未提交) | `MobileSyncCredentialModal.tsx` 主 QR 文案 + 次要卡片; i18n + 单测同步; 9/9 + 80/513 通过 |
| 阶段 4: iOS 模板 + Android 文档 | ⏳ 待办 | — | `docs/integrations/ios-shortcut.md` + 模板更新 (仓库外) |

## 会话日志

### 2026-05-18 (阶段 3B)

- **i18n 文案** (`src/i18n/locales/{en-US,zh-CN}.json`):
  - `qr.label` / `qr.alt` 改 auto-fill 语义; 新增 `qr.help` 副文案 (告诉用户快捷指令会自动读 url/user/pwd, 无需手动输)。
  - 新增 `installShortcut.{title, body}` 二级卡片文案; 中途加的 `installShortcut.cta` 又删除 —— 桌面端打开 iCloud 链接无意义，CredentialField 自带 copy 就够。
  - `installUrl.label` 从 "Or copy the install link" / "或复制安装链接" 改为 "Install link (one-time)" / "安装链接 (一次性)", 体现"一次性入口"语义。
- **`MobileSyncCredentialModal.tsx`** iOS tab:
  - 主 QR 区图源不变 (`data:image/png;base64,${qrCodePngBase64}`) —— 后端阶段 2 已切到编码 connect URI, 前端无需感知具体编了什么，只需更新 alt/label 让 UX 语义对齐。新增 help 副文案放在 QR 下方。
  - 新增二级"首次安装"卡片包裹 install URL 字段 (border + bg-card/50 突出降级), 沿用 CredentialField 自带 copy 按钮。
  - 顶部组件注释更新 iOS tab 描述：主操作 = connect URI QR, 次要 = 首次安装。明确"桌面端打开 iCloud 链接无意义，只复制即可，在 iPhone Safari 粘贴"。
- **单测** (`MobileSyncCredentialModal.test.tsx`):
  - mockPayload 加 `connectUri: 'uniclipboard://connect?v=1&svc=mobile-sync&p=...'` 字段 (DTO 阶段 2 已有，之前测试缺字段是 TS 隐式默认 tolerated)。
  - 新增 `renders the QR as the primary auto-fill action with the new alt text`: 断言 QR alt 文案 = "QR code that auto-fills the sync credentials", img src 来自 PNG base64, label 文案存在。
  - 新增 `shows the install-shortcut secondary card with the install URL`: 断言"首次安装"标题 + install link label + install URL 字面值三件套都可见。
- **测试结果**:
  - 定向：`bun run test src/components/device/__tests__/MobileSyncCredentialModal.test.tsx` → 9/9 通过 (原 7 + 新 2)
  - 全套：`bun run test --run` → 80 文件 / 513 测试 OK (原 511 + 新 2), 无回归
  - Lint: `npx eslint <两个改动 tsx>` → 0 error / 0 warning

### 2026-05-18 (阶段 3A)

- **新增** `src/lib/mobileSyncConnectUri.ts` (303 行):
  - `buildConnectUri(baseUrl, user, pwd, other)` + `parseConnectUri(qrText)` 一对纯函数，与 Rust 端字节级镜像。
  - `ConnectUriError extends Error` 含 `code` 字段 (7 个 `ConnectUriErrorCode` 联合常量); `MISSING_FIELD` 携带 `field`, `URI_TOO_LONG` 携带 `len/max`, `PAYLOAD_DECODE_FAILED` 携带底层 detail —— 比单纯字符串错误更便于前端 i18n 文案 + UI 展示。
  - 严格按 `findings.md` 列出的 6 条字节稳定性约束实现：
    1. 显式按 v/url/user/pwd/o 顺序构造对象，不依赖 JSON.stringify 隐式键顺序。
    2. `o` 内部键排序后逐项插入 `Record<string, string>` —— 浏览器 V8/JSC 保证 JSON.stringify 按插入顺序输出字符串键。
    3. JSON.stringify 默认 minify, 无空白。
    4. 空 `o` 跳过，避免 `"o":{}` 让 base64 漂移。
    5. base64url-no-pad: `btoa` 后 `+→-`, `/→_`, 去 `=` padding。
    6. UTF-8: `TextEncoder` / `TextDecoder('utf-8', { fatal: true })`。
  - `bytesToBase64Url` 用 chunked `String.fromCharCode` 拼 binary string, 防大数组爆栈 (connect URI 实际 ≤ 800 字符，远不到，但保留稳健性)。
  - `coercePayload()` 把 `JSON.parse` 出的 `unknown` narrow 到 `ConnectPayload`: `v` 非整数 → `UNSUPPORTED_VERSION`(与 Rust serde 行为一致); 未识别 `o.*` 键宽松保留 (规范 §3.2 前向兼容); 非 string 的 `o` 字段静默丢弃避免类型污染。
- **新增** `src/lib/__tests__/mobileSyncConnectUri.test.ts` (23 测试，实跑 22 pass 1 implicit):
  - happy-path: golden URI 字节级 ===  Rust `GOLDEN_URI`
  - 空 other → JSON 无 `"o"`(回归保护)
  - `o` 键即使乱序传入也强制字典序
  - 5 个 build 负例：empty url/user/pwd, 非 http url, 超长 URI(带 `len/max` 字段断言)
  - 6 个 parse 负例 + 4 个边界负例，与 Rust §7.2 + `parse_rejects_*` 一一镜像
  - parse 宽松保留未知 `o.future_key` (前向兼容)
  - build → parse round-trip 含 Unicode label "我的 iPhone"
- **跨语言契约成立**: golden vector 字符串在 Rust `connect_uri.rs:282` 与 TS test 字面量字节相同; 任一侧序列化漂移会让 `emits the golden URI byte-for-byte` 立刻失败。
- **lint 顺手 fix**: eslint 抓到 import 顺序问题 (plugin `import-x/order` 要求 vitest + 本地 import 不空行), `npx eslint --fix` 一行修好。`bun run lint` 整仓跑被 docs-site 的 Next.js 子项目阻塞 (缺 `eslint-config-next`), 单跑两个新文件 0 error。
- **测试结果**:
  - `bun run test src/lib/__tests__/mobileSyncConnectUri.test.ts` → 22 / 22
  - `bun run test --run`(全套) → 80 文件 / 511 测试 OK, 无回归。

### 2026-05-18 (阶段 2)

- **阶段 2 落地**:
  - `register_device.rs`:
    - 加 `use super::connect_uri::{build_mobile_sync_connect_uri, ConnectUriError, ConnectUriOther};` (走 `pub(crate)` 同模块，不破坏 `uc-application` §11.4 外部边界)。
    - `RegisterMobileShortcutDeviceOutput` 新增 `connect_uri: String` 字段; `install_url` 保留 (降级为"首次安装"次要入口)。
    - `execute()` 在 device save + analytics emit 之后组装 `ConnectUriOther { label, did, proto:"syncclipboard", install:None }` → `build_mobile_sync_connect_uri(...)` → 翻译错误 → `render_qr_code(&connect_uri)`。
    - 新增 `translate_connect_uri_error()` helper: `UriTooLong → QrRenderFailed(带 len/max)`; 其余 6 个变体走 `unexpected: {err}` catch-all(理论上不可能触发 — base_url 由 format! 拼出、user/pwd 走 minter 或前置校验)。
    - 函数 `render_install_qr` 重命名为 `render_qr_code`(语义变了，只有一个调用方，跨文件零外溢)。
  - 测试：现有 22 个 `mod tests` 用例全绿 + 4 个新增：
    - `auto_path_returns_minter_credentials_and_install_url` 扩展：用 `parse_mobile_sync_connect_uri` 反向解出 url/user/pwd + label/did/proto, install 字段为 None。
    - `qr_content_follows_connect_uri_not_install_url`: 单独跑一遍 `render_qr_code(SYNC_CLIPBOARD_EX_INSTALL_URL)`, 断言 use case 输出 PNG/ASCII 字节都 ≠ install_url 编码 — 这是阶段 2 之前→之后的回归保护。
    - `translates_uri_too_long_to_qr_render_failed_with_hint`: 直接测翻译函数，避开 end-to-end 算术。
    - `translates_other_connect_uri_errors_to_qr_render_failed`: 6 个 catch-all 变体逐一断言带 `unexpected` 前缀 + 保留原错误描述。
  - `uc-tauri/commands/mobile_sync.rs`:
    - `RegisterMobileDeviceResult` 加 `pub connect_uri: String` (camelCase 透传走 specta::Type + serde rename_all = "camelCase")。
    - `From<RegisterMobileShortcutDeviceOutput>` 字段透传 `connect_uri: out.connect_uri`。
    - 2 个测试更新：`register_result_qr_is_base64_encoded` 加 connect_uri/install_url 断言; 新增 `register_result_serializes_connect_uri_camel_case` 直接断 wire 上字段名为 `connectUri`。
  - bindings 自动重生：`cargo test -p uc-tauri --test specta_export` 写出新 `src/lib/ipc-bindings.generated.ts`, `RegisterMobileDeviceResult` 多出 `connectUri: string` 字段，含 doc-comment。
- **顺手收尾**:
  - 阶段 1 提交后留下 12 个 dead-code 警告 (整个 connect_uri 模块没人消费)。阶段 2 让 `build/ConnectUriOther/ConnectPayload/ConnectUriError(部分变体)/常量` 全部被 register_device.rs 消费，自动消除 10 个。
  - 剩余 2 个警告 (parse 函数 + 3 个 parse-only error 变体) 是预留供:(a) 单测 round-trip; (b) 未来 v2 daemon 接收侧; (c) 跨语言契约对照 — 加 `#[allow(dead_code)]` + 注释明确意图，不静默 lint。
- **测试结果**: `cargo test -p uc-application -p uc-tauri` 全绿：
  - `uc-application` lib: 529 测试 OK (含 register_device 24 + connect_uri 22)
  - `uc-tauri` lib: 35 测试 OK
  - `uc-tauri` mobile_sync_dto: 10 测试 OK (含 2 个新增)
  - `uc-tauri --test specta_export`: 1 测试 OK (bindings 写盘成功)

### 2026-05-18 (阶段 0-1)

- **需求分析与拆解**: 基于 issue #789 文档，把工作拆成 5 个独立可合入的阶段 (0-4)。
- **三个开放问题用户裁定**:
  1. 编解码模块归 `uc-application` ✅
  2. `o` 字段采用"生成侧白名单 + 解析侧宽松" ✅
  3. `install_url` DTO 字段保留 ✅
- **scheme alias 决定**: 用户裁定仅保留 `uniclipboard://`, 不接受 `uniclip://` 别名。
- **阶段 0 完成**: 写入 `docs/architecture/mobile-sync-connect-uri.md`, §7 golden vector 用 Python base64 实算独立验证 happy-path 与负例 5/6 的字节准确性。
- **阶段 1 完成**: `connect_uri.rs` + 22 单元测试通过。
  - 首次测试发现 `parse_rejects_missing_pwd` 失败 (serde 直接报错走 `PayloadDecodeFailed`), 加 `#[serde(default)]` 后归并到 `MissingField`, 与规范 §4.2 错误码归并对齐 — **决策已写入 task_plan.md**。
  - URL crate probe 实测：`uniclipboard://connect?...` 在 `url 2.x` 下正常解析 host/query, 无需手写 parser。
- **提交**: `ec59277b feat(mobile-sync): add connect URI v1 protocol spec and codec` — 3 files / 983 insertions。pre-commit hook 跑了 cargo fmt + autocorrect-fix, 不影响功能。
- **planning 文件落盘**: 按项目 `.planning/phases/NNN-slug/` 惯例创建 099 目录，三件套就位。

## 错误日志

| 错误 | 阶段 | 解决方式 |
|---|---|---|
| serde 在 `pwd` 字段缺失时直接报 `PayloadDecodeFailed`, 与规范 `MISSING_FIELD` 语义不符 | 阶段 1 测试 | 给 url/user/pwd 字段加 `#[serde(default)]`, 让 serde 兜底空字符串，后置 `MissingField` 检查统一处理 |
| `git add` 找不到 `docs/...` 文件 (cwd 在 `src-tauri/` 下) | 阶段 1 commit | 改用 `git -C <repo-root>` 显式指定仓库根 |
| 第一版 "translates_connect_uri_too_long" 走 end-to-end 路径 (MAX_LABEL_LEN/USERNAME/PASSWORD 全顶满), 算 base64 膨胀后 URI 仍只到 ~840 字符，边界脆弱且需多字节字符堆才能稳定触发 | 阶段 2 测试 | 改为直接调 `translate_connect_uri_error(ConnectUriError::UriTooLong{...})`, 不走 use case, 测翻译函数本身。end-to-end 太长由规范文档 §2 兜底 |
| `cargo test -p uc-tauri --test specta_export` 跑前显示 `parse_mobile_sync_connect_uri` 与 3 个 parse-only 变体 dead-code | 阶段 2 警告清理 | 加 `#[allow(dead_code)]` + 解释为何保留 (单测/v2/跨语言契约), 不静默 lint |

## 决策日志

- 2026-05-18: 三个开放问题 (模块归属 / `o` 白名单 / `install_url` 保留) 按用户裁定。
- 2026-05-18: 单一 scheme — 仅 `uniclipboard://`, 拒绝 `uniclip://` alias。
- 2026-05-18: `MissingField` 归并语义 — serde struct 字段加 `#[serde(default)]`。
- 2026-05-18: Golden vector 选用 `proto`/`label`/`did` 三个 `o` 键，URI 259 字符。
- 2026-05-18: 编解码模块归 `uc-application` 而非 `uc-core` — 它服务于 use case, payload schema 属应用层契约。
- 2026-05-18 (阶段 2): `install_url` DTO 字段保留，但 QR 渲染对象切换为 `connect_uri`。前端阶段 3B 把 install_url 降级到二级"首次安装"卡片。
- 2026-05-18 (阶段 2): `o.install` 字段在阶段 2 暂留空，等阶段 4 真机走通后再决定是否塞 iCloud 链接到 payload(规避在两处维护同一份 URL)。
- 2026-05-18 (阶段 2): 函数名 `render_install_qr` 改为 `render_qr_code` — 旧名误导，现在它编任意 URI。crate 内零外溢，不影响 §11.4 边界。
- 2026-05-18 (阶段 2): `ConnectUriError → RegisterMobileShortcutDeviceError` 全部翻译为 `QrRenderFailed`(复用现有变体), 不新增错误码。`UriTooLong` 带 `len/max`; 其余 catch-all 带 `unexpected:` 前缀供日志排障。
- 2026-05-18 (阶段 3B): QR `<img src>` 仍是 `data:image/png;base64,${qrCodePngBase64}` 字段不变 —— 后端 DTO 已切，前端只更新 alt/label 文案。
- 2026-05-18 (阶段 3B): install URL 不加 "Open in Shortcuts" CTA, 沿用 CredentialField 自带 copy; 同时把刚加的 `installShortcut.cta` i18n 文案删除避免孤儿键。
- 2026-05-18 (阶段 3B): 前端单测只断言 UI 结构 (alt 文案 + 次要卡片可见), 不跑跨语言 byte-level 比对 —— 那是阶段 3A 的 `mobileSyncConnectUri.test.ts` 职责。

## 下一步动作

阶段 2 (`3756c84e`) 已提交; 阶段 3A + 3B 本地完成待提交 (可单独 PR, 都不影响阶段 4 闭环 — UI 已就位但端到端需快捷指令模板配合)。

启动 **阶段 4** (iOS 快捷指令模板 + Android 客户端文档):
1. `docs/integrations/ios-shortcut.md` (英文): 两阶段 UX 流程 (首次安装 → 后续扫码), 快捷指令需新增的步骤 (URL Trigger → 取 `p` query → base64url 解码 → JSON 取 url/user/pwd → 写三个文本变量), 错误处理建议。
2. `docs/integrations/android-syncclipboard.md` (英文，可选): Intent filter 示例 + 字段映射表 + 兼容客户端 checklist。
3. 仓库外：iOS 快捷指令模板更新 + 重新签名 + 通过 iCloud 分享，可能需要更新 `SYNC_CLIPBOARD_EX_INSTALL_URL` 常量 (取决于是否复用旧链接)。
4. 真机 UAT: iPhone(iOS 17+) 一次扫码 → 快捷指令自动写入三栏 → 触发同步 → desktop entry 列表出现新增项。

阶段 4 详细任务见 `task_plan.md`。
