# Findings: 099 · 移动端扫码接入协议

## 关键代码锚点 (已实地核对)

### `uc-application` use case

- `src-tauri/crates/uc-application/src/usecases/mobile_sync/register_device.rs`
  - `RegisterMobileShortcutDeviceUseCase::execute()` — 当前出口
  - 字段 `SYNC_CLIPBOARD_EX_INSTALL_URL` (常量，行 177-178)
  - `RegisterMobileShortcutDeviceOutput` (行 60-81): `device / base_url / username / password / install_url / qr_code_png_bytes / qr_code_ascii`
  - `RegisterMobileShortcutDeviceError` (行 84-154): 现有 16 个变体，含 `QrRenderFailed(String)` 可复用
  - `render_install_qr()` (行 491-514): qrcode + image::Luma 渲染管线，**阶段 2 复用此函数，只换输入字符串**
  - 现有测试覆盖：22 个 `mod tests` 用例，阶段 2 需更新 happy-path 断言中的 `install_url` 检查

- `src-tauri/crates/uc-application/src/usecases/mobile_sync/connect_uri.rs` (新，阶段 1)
  - `build_mobile_sync_connect_uri(base_url, user, pwd, other) -> Result<String, ConnectUriError>`
  - `ConnectUriOther { label, did, proto, install }` (build-side 白名单)
  - `ConnectPayload { v, url, user, pwd, o }` (解码目标，`o` 为 `BTreeMap<String,String>`)
  - `ConnectUriError` 七变体：`InvalidScheme / UnsupportedVersion / UnsupportedService / PayloadDecodeFailed / MissingField / InvalidUrl / UriTooLong`
  - `URI_MAX_LEN: usize = 800` 常量 (public crate)
  - 22 个测试，含 golden `GOLDEN_URI` 常量 — **TS 阶段 3 测试可直接复制此字符串**

### Tauri DTO 与 facade 转发

- `src-tauri/crates/uc-tauri/src/commands/mobile_sync.rs` — DTO 定义
  - 阶段 2 在 register 对应的 output struct 上加 `connectUri: String` (camelCase, specta)
- 上层 facade: `uc-application/src/facade/mobile_sync/` (`§11.4` 约定的对外入口)
  - 阶段 2 改动是否需要 facade 层 method 签名调整，待启动阶段 2 时核对

### 前端

- `src/components/device/MobileSyncCredentialModal.tsx` — 凭据弹窗主体
  - iOS tab 的 QR 渲染处 (阶段 3B 主战场)
  - iCloud 安装链接展示位 — 阶段 3B 降级为次要卡片
- `src/components/device/AddMobileSyncDeviceDialog.tsx` — 添加设备入口对话框 (调用 register, 接 DTO 输出展示)
- `src/lib/` — 共享前端 lib 目录，阶段 3A 新增 `mobileSyncConnectUri.ts`
- `src/lib/__tests__/` — Vitest 测试目录

### iOS 快捷指令 (本仓库外)

- 当前 iCloud 安装链接：`https://www.icloud.com/shortcuts/9c2319d7d6404521b941271e89194f30`
  - 定义在 `register_device.rs:177-178` 的 `SYNC_CLIPBOARD_EX_INSTALL_URL` 常量
  - **阶段 4 是否需要更新此常量**: 取决于模板是否能就地升级 (同 iCloud 链接) 还是必须重新生成新链接

## 规范文档锚点

- 单一真相：`docs/architecture/mobile-sync-connect-uri.md`
- 关键章节：
  - §2 URI 形态 + 800 字符上限
  - §3 v1 payload schema(字段表 + `o` 白名单 + 字节稳定性约定)
  - §4 解析算法 + 错误码
  - §5 安全约束
  - §7 Golden test vector
  - §10 v2 演进预案

## 已验证的实测结论

- `url::Url::parse("uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ")` 在 `url 2.x` 下正常解析：`scheme="uniclipboard"`, `host=Some("connect")`, `query_pairs()` 正常迭代 — **不需要手写 URI parser**。
- Python `base64.urlsafe_b64encode().rstrip(b'=')` 与 Rust `base64::engine::general_purpose::URL_SAFE_NO_PAD::encode()` 输出字节相同 — 三方编码器可用作 golden vector 的独立第三方验证。
- 规范 §7.1 happy-path URI 经 Rust 编码器实测输出 = 259 字符，与 Python 实算一致，远低于 800 字符上限。
- serde 默认序列化 struct 时按字段定义顺序输出; `BTreeMap` 序列化为字典序。两者合起来让 Rust 与 TS(将用 `Object.keys().sort()` 显式排序) 字节级一致。

## 跨语言字节一致性的关键约束

为让 Rust 与 TS 编码器字节相等 (让 golden vector 可双向复用), 必须满足：

1. **JSON 字段顺序固定**: `v / url / user / pwd / o` (Rust 用 serde 默认按字段定义顺序; TS 必须 **手动** 按此顺序构造对象 — `JSON.stringify` 在 V8 实现里按"插入顺序"输出，但规范不保证 — 阶段 3A 需要写 helper 显式按字段构造对象)
2. **`o` 键字典序**: Rust 用 `BTreeMap`; TS 用 `Object.keys(other).sort().reduce(...)`
3. **JSON minify**: 双方都不加空白
4. **空 `o` 跳过序列化**: Rust 用 `skip_serializing_if = "BTreeMap::is_empty"`; TS 用 `if (Object.keys(o).length > 0) result.o = o`
5. **base64url-no-pad**: Rust 用 `URL_SAFE_NO_PAD`; TS 用 `btoa(...).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')`
6. **UTF-8 编码**: Rust 字符串本就是 UTF-8; TS 用 `new TextEncoder().encode(s)` 拿到 Uint8Array, 再 base64

阶段 3A 的实现必须按上述顺序逐项对齐，任一项漂移都会让 TS 输出与 Rust GOLDEN_URI 不匹配。

## 历史背景 / 现状

- `register_device.rs` 模块顶部注释提到 v3 切到 SyncClipboard 兼容路径后，不再维护自建 `.shortcut` 模板; 用户安装 Apple 签名的 iCloud 链接。本协议 **不挑战这个决策**, 只补全"扫码后填三栏"的缺口。
- `.context/mobile-sync/SPEC.md` §14.2 + `findings.md` v3 段落是 mobile-sync 子系统的总规范，本协议是该规范"二维码内容"一项的细化，不替换主规范。
- `get_settings.rs` 中曾有 `uniclip://config?u=...&t=...` 与 `TokenInjected` 的 dead code / 注释，它们是被本协议 **取代** 的早期方向，阶段 2 实现时可顺手删除注释保持代码整洁。

## 待澄清

- 阶段 2 时确认 facade 层是否需要 method 签名变更，还是 DTO 改动可单纯落在 commands 层。
- 阶段 4 是否替换 `SYNC_CLIPBOARD_EX_INSTALL_URL` 常量，取决于模板更新方式 — 启动阶段 4 时与产品 / 模板维护者对齐。
