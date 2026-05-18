# Progress: 099 · 移动端扫码接入协议

## 阶段总览

| 阶段 | 状态 | Commit / PR | 备注 |
|---|---|---|---|
| 阶段 0: 协议规范文档 | ✅ 完成 | `ec59277b` | `docs/architecture/mobile-sync-connect-uri.md`, 含 §7 golden vector |
| 阶段 1: Rust 编解码 + 22 测试 | ✅ 完成 | `ec59277b` | `connect_uri.rs`, 与规范字节一致 |
| 阶段 2: Rust use case + DTO | ✅ 完成 | `3756c84e` | `register_device.rs` 改 QR 内容; `mobile_sync.rs` DTO 加 `connectUri`; bindings 自动重生; 顺手清掉 phase 1 留下的 10 个 dead-code 警告 |
| 阶段 3A: TS 解析器 + Vitest | ✅ 完成 | `23452385` | `src/lib/mobileSyncConnectUri.ts` + 跨语言 golden vector 22 测试 |
| 阶段 3B: 凭据弹窗 UI | ✅ 完成 | `3b220f75` | `MobileSyncCredentialModal.tsx` 主 QR 文案 + 次要卡片; i18n + 单测同步; 9/9 + 80/513 通过 |
| 阶段 4A: iOS App + Shortcut 集成文档 | ✅ 完成 | `aeb85dd5` | `docs/integrations/{ios-app-connect-uri,ios-shortcut}.md` + spec §9/§11 重排 |
| 阶段 4B: iOS App 仓库 Swift 落地 | ✅ 完成 (跨仓库) | — | `/Users/mark/MyProjects/iOSApp/UniClipboard`, 用户真机扫码测试通过 |
| 阶段 5: 凭据弹窗 Tab 重构 | ✅ 完成 | (本地未提交) | 按"接入方式"分 (扫码接入 / 安装快捷指令); install URL 独立 QR (后端 + DTO + bindings + UI + i18n + 单测) |
| 阶段 4C: Shortcut 模板更新 + iCloud | ⏳ 仓库外手工 | — | 用户在 Shortcuts.app 操作 |

## 会话日志

### 2026-05-18 (阶段 5: 凭据弹窗 Tab 重构 + install URL 独立 QR)

- **触发**: 用户在 iOS App 4B 真机测试通过后追问"现有 QR 是否也能给 Android 客户端用？" — 答案是协议层完全兼容 (connect URI 平台无关，HTTP wire 也是同一套), 但前端 Modal 当前的 Android tab 只显示文字说明、没 QR, 既不准也容易让 Android 用户误以为不支持。决定按"接入方式"重组 tabs。
- **后端** `src-tauri/crates/uc-application/.../register_device.rs`:
  - `RegisterMobileShortcutDeviceOutput` 新增 `install_qr_code_png_bytes: Vec<u8>` 字段，与既有 `qr_code_png_bytes` (编 connect URI) 对称。
  - `execute()` 第二次调 `render_qr_code(&install_url)` 渲染 install URL QR — 共用同一管线，防止两个 QR 视觉/编码不一致。
  - 两个测试增强：`auto_path_returns_minter_credentials_and_install_url` 加 install QR PNG magic 断言; `qr_content_follows_connect_uri_not_install_url` 加对称断言 (install QR == install URL 编码字节，主 QR ≠ install URL 编码) — 防止字段串位。
- **Tauri DTO** `src-tauri/crates/uc-tauri/src/commands/mobile_sync.rs`:
  - `RegisterMobileDeviceResult` 加 `install_qr_code_png_base64: String` (specta + serde camelCase 自动透出 `installQrCodePngBase64`)。
  - `From<Output>` 透传 `BASE64.encode(out.install_qr_code_png_bytes)`。
  - 2 个 DTO 单测扩展：`register_result_qr_is_base64_encoded` 加 install QR base64 断言; `register_result_serializes_connect_uri_camel_case` 加 wire 上 `"installQrCodePngBase64":""` camelCase 断言。
  - `specta_export` 重生 `src/lib/ipc-bindings.generated.ts`, 自动多出字段 + doc-comment。
- **前端 i18n** `src/i18n/locales/{en-US,zh-CN}.json`:
  - 删除旧 keys: `platforms.{ios,android}` / 顶层 `qr.*` / `installShortcut.{title,body}` / `installUrl.label` / `android.instructions`。
  - 新增分组化 keys: `platforms.{scan,shortcut}` ("扫码接入"/"安装快捷指令", "Scan to add"/"Install Shortcut") + `scan.qr.{label,alt,help}` (help 一行说清"iOS App + Android 第三方应用都能扫") + `shortcut.{title,body}` + `shortcut.qr.{label,alt}` + `shortcut.linkLabel`。
- **前端 UI** `src/components/device/MobileSyncCredentialModal.tsx`:
  - `Platform = 'ios' | 'android'` → `OnboardingTab = 'scan' | 'shortcut'`, 默认 `scan`; `resetLocalState` 同步改。
  - Tab A "扫码接入" (默认): 主 connect URI QR (`payload.qrCodePngBase64`) + 副 help 文案 (含 Android 兼容客户端说明), 即原 iOS tab 主 QR 区，但删除了次要"首次安装"卡片。
  - Tab B "安装快捷指令": 顶部说明卡 + install URL QR (`payload.installQrCodePngBase64`) + CredentialField 显示 install URL 文本 (含 copy 按钮)。
  - 组件顶部注释完整重写 — 从"按平台分"叙述切到"按接入方式分", 明确两个 QR 各自内容 + 用途。
- **前端单测** `src/components/device/__tests__/MobileSyncCredentialModal.test.tsx`:
  - mockPayload 加 `installQrCodePngBase64: 'aW5zdGFsbFFy'` (与主 QR 'iVBORw0KGgo=' 故意不同 — 防字段串位)。
  - 替换原 3B 加的 2 条 UI 断言：
    - `defaults to the Scan tab with the connect-URI QR`: 断 tab triggers 两个 label 存在，默认渲 connect URI QR (src 来自主 base64), 新 Android-compatible help 文案可见。
    - `switches to the Install Shortcut tab and shows the install-URL QR + link`: click Tab B → 顶部说明卡 + linkLabel + install URL 文本 + install QR (src 来自 install base64) 都可见。
  - 现有 7 条 close-behavior 用例 (Done / Discard / Escape / acknowledgement / null payload / 双击 X) 全部保留，行为零变。
- **测试结果**:
  - `cargo test -p uc-application --lib`: 529 OK
  - `cargo test -p uc-tauri --lib`: 35 OK
  - `cargo test -p uc-tauri --lib commands::mobile_sync`: 18 OK (含 2 个 DTO 单测扩展)
  - `cargo test -p uc-tauri --test specta_export`: 1 OK
  - `bun run test src/components/device/__tests__/MobileSyncCredentialModal.test.tsx`: 9 OK (原 7 + 新 2 替换原 2)
  - `bun run test --run`: 80 文件 / 513 OK, 无回归
  - `npx eslint <两个改动 tsx>`: 0 error / 0 warning

### 2026-05-18 (阶段 4 范围澄清 + 4A 文档落地)

- **关键澄清**: 用户指出"接入端"的真实预期是 iOS 原生 App (走 URL scheme + .onOpenURL，让系统相机扫码即可跳 App 自动填表),**不是** 先前理解的 SyncClipboard 快捷指令。Shortcut 路径降为兜底但仍维护; Android 文档不写 (spec §9.3 已覆盖第三方实现契约)。
- **survey iOS App** (`/Users/mark/MyProjects/iOSApp/UniClipboard`): SwiftUI App / iOS 26.2 / Swift 5 MainActor / SwiftPM 测试; 已有 `SyncClipboardClient` / `AppSettings` / `ServerConfig` / `SetupFlowView` (Welcome → ServerForm → AutoSwitch) / `QRScannerView` + `ServerQRPayload` (JSON / URL-userinfo); **没有** URL scheme 注册，**没有** .onOpenURL handler。`ServerQRPayload` 与 connect URI 不兼容 (前者不带 scheme，后者是 `uniclipboard://` URL) — 决策保留 legacy 格式，新增 `uniclipboard://` 分支共存。
- **新增 `docs/integrations/ios-app-connect-uri.md`** (~430 行，英文):
  - §1-2 Why 与 URL scheme 注册 (Xcode 26 UI 路径; CFBundleURLTypes 不能走 INFOPLIST_KEY_*; plutil 验证命令)。
  - §3 `.onOpenURL` 挂 WindowGroup 根 + AppViewModel.handleIncomingURL 路由 sketch。
  - §4 Swift `ConnectURI.Payload / ParseError` 类型 + 6 步 parser 完整 Swift 代码 (含 base64url decode helper, URL scheme http(s) 校验，MissingField 归并语义); 故意不实现 encoder (desktop 是唯一颁发方)。
  - §5 跨语言 golden test (复用 spec §7.1 字面值，swift-testing macro 风格，4 个 §7.2 负例)。
  - §6 UX 路由表 (SetupFlow 空态 / 已有 server / 模态在线 三态分别响应)。
  - §7 错误码 → i18n 文案映射建议 (en + zh-Hans)。
  - §8 `ServerQRPayload.parse` 入口统一改造 (legacy 与新 connect URI 共存)。
  - §9 `xcrun simctl openurl` 验证命令，带 §7.1 golden URI 字面值。
  - §10 维护规约 (o.* 新键无需 iOS 改; v bump 协调; golden test 是 drift detector)。
- **新增 `docs/integrations/ios-shortcut.md`** (~150 行，英文):
  - §1 Why 兜底定位。
  - §2 两阶段 UX (不变)。
  - §3 22 步 Shortcut Actions 表 (Receive Input / If / Get URLs / Match Text / Replace Text / Calculate padding / Base64 Decode / Get Dictionary 等); 关键 base64url → base64 6 步映射 (- → +, _ → /, 重补 padding) 与 encoder 字节一致。
  - §4 spec §7.1 golden URI 自检 + 故障定位 checklist。
  - §5 iCloud 分享 + `SYNC_CLIPBOARD_EX_INSTALL_URL` 常量更新触发条件。
  - §6 退役条件 (>95% 走原生 App)。
- **更新 `docs/architecture/mobile-sync-connect-uri.md`**:
  - §9 重排为 delivery-priority order: §9.1 = Native iOS App (primary), §9.2 = Shortcut (fallback, still maintained), §9.3 = Android / third-party (spec 即契约)。
  - §11 实现位置表新增 2 行：ios-app-connect-uri.md / ios-shortcut.md。
- **下游影响**: 4B (iOS App 仓库 Swift 落地) 与 4C (Shortcut 模板手工更新) 跨仓库/仓库外，本桌面仓库不参与。

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
- 2026-05-18 (阶段 4 范围): iOS 原生 App (URL scheme + .onOpenURL) 是主路径，SyncClipboard 快捷指令降为兜底但仍维护; Android 文档不写。
- 2026-05-18 (阶段 4A): iOS App 既有 `ServerQRPayload` 与 connect URI 不兼容 — 保留 legacy 在 `ServerQRPayload.parse` 入口新增 `uniclipboard://` 分支并存，而非弃用旧格式。
- 2026-05-18 (阶段 4A): iOS Swift parser 故意不实现 encoder — desktop 是唯一颁发方，iOS encoder = dead code + drift 风险; golden test 三方独立断言同一字面值就是 drift detector。
- 2026-05-18 (阶段 4A): URL scheme 注册走 Xcode UI (Target → Info → URL Types) 而非 INFOPLIST_KEY_* — Xcode 26 generated-Info.plist 模型下 `array<dict>` 无法用 INFOPLIST_KEY_* 表达，PlistBuddy build phase 是更重的备选。
- 2026-05-18 (阶段 4A): .onOpenURL 挂 `UniClipboardApp` 的 `WindowGroup` 根 (非 ContentView) — 防 SetupFlow/TabView 状态切换重新挂载时丢消息。
- 2026-05-18 (阶段 5): Tab 按"接入方式"分而非"平台", 反映"凭据/QR 协议平台无关"的实际事实，同时治掉旧 Android tab 啥都没有的死角。
- 2026-05-18 (阶段 5): install URL QR 走后端 `render_qr_code` 二次渲染 — 与 connect URI QR 共用同一管线 (视觉/编码风格一致), 前端零新依赖，缺点是 DTO 多带 ~600 字节 PNG (可接受)。
- 2026-05-18 (阶段 5): mockPayload 两个 QR base64 字面值故意不同 — 防止前端把 `installQrCodePngBase64` ↔ `qrCodePngBase64` 串位 (字段名相近，类型相同，复制粘贴失误是真实风险); 后端 use case 测试也加了对称断言。
- 2026-05-18 (阶段 5): Tab labels 用"动作描述" ("扫码接入"/"安装快捷指令") 而非协议名 ("Connect URI"/"iOS Shortcut") — 用户视角更直观。

## 下一步动作

阶段 0-4A 全部提交; 阶段 4B (iOS App 仓库 Swift) 用户已完成并真机测试通过; 阶段 5 (凭据弹窗 Tab 重构) 本地完成待提交。本桌面仓库剩余仅一项可选动作：

- **阶段 4C** (仓库外手工，可选): 按 `docs/integrations/ios-shortcut.md` §3 把 22 个 actions 加进 SyncClipboard 快捷指令模板; iCloud 重新分享; 如新链接 ≠ 现常量，另开小 PR 改 `SYNC_CLIPBOARD_EX_INSTALL_URL`。
  - 优先级低：iOS App 主路径已通，快捷指令是兜底; 没装 App 的兼容路径才需要。
  - 模板更新后，阶段 5 加的"安装快捷指令" tab 里的 install URL QR 就能让用户用 iPhone 相机直接扫装 (端到端跑通)。

后续可能的协议演进 (留待新 phase / 新 issue):
- spec §10 提到的 `o.exp` / `o.token` / `o.push` 等 v1 之外的字段，若需要再独立规划。
- 阶段 5 新增的 `installQrCodePngBase64` DTO 字段若未来想用 `connect_uri.payload.o.install` 下沉 (在 connect URI 里内嵌 install link), 是 spec §3.2 的扩展点，不破坏 v1 协议。
