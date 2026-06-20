# Issue #1110 — 配置导入/导出 (跨 portable ↔ installer 迁移) 设计文档

- 状态：已实现;2026-06-20 维护者复盘后做了两处易用性修订 (见第 0 节),已落地
- 日期:2026-06-19(2026-06-20 修订)
- 关联 issue:#1110「允许配置数据互通或导出」
- 方案选型：**应用内「导出/导入配置包」**(定位为 **迁移/搬家**,非多机备份),已与维护者确认

---

## 0. 2026-06-20 修订 (易用性，已落地)

两处优化，覆盖了下文部分原始决策。原文保留以记录推演过程，本节为当前真相。

### 0.1 导出不再要用户输口令 (覆盖 §3.2 / §3.3 / Q? 中"export_password")

- **现状**:导出 daemon 直接用它一直持有的 **KEK**(`kek:v1:profile:{id}`,存在 secure storage) 作为包的 AEAD 密钥，把 keyslot 的 salt / kdf 写进包头。导入端 `bundle::open(passphrase, ...)` 用 `Argon2id(passphrase, 包头salt, 包头kdf)` 现场重算出同一个 KEK 解包 —— 因为 `KEK == Argon2id(space passphrase, keyslot.salt, keyslot.kdf)` 是既有不变量。
- **根因**:daemon 解锁后只在内存留 `master_key`,**从不保留明文口令**(静默解锁路径 `try_resume_session` 连口令都没见过)。所以"默认用创建时的口令"不能靠"复用缓存口令"实现;改用 daemon 一直持有的 KEK 等价达成"包被 space 口令保护",且导出端省掉一次 Argon2。
- **影响**:导出全链路删掉 `password` 参数 (core port → facade → webserver DTO/handler → daemon-client → tauri command → 前端)。导出 UI 从"弹口令框"变成一键。导入端 **密码学零改动**:仍输入 space 口令，`open` 照常工作。
- **副作用**:KEK 恒被携带 (导出强制要求 KEK 在场，缺则报错而非产出谁也打不开的包),故 `unlock_required_after_apply` 恒为 false，导入重启后自动解锁。

### 0.2 导入即整体替换，去掉"必须未初始化"前置 (覆盖 §2 非目标 / §3.4 / Q? AlreadyInitialized)

- **现状**:`stage_import` 不再检查目标机是否已初始化，`ConfigMigrationError::AlreadyInitialized` 变体已删除。boot 期 `apply_pending_import` 本就是幂等覆盖 (文件 copy 覆盖 + secret set 覆盖),所以"导入=替换"天然成立;额外补了 **替换已有库时清理陈旧 SQLite `-wal`/`-shm` 旁文件**(否则新快照会被旧 WAL 污染)。
- **理由**:用户不该被要求"先去找重置按钮、手动重置、再导入"。导入本就是设备搬家 (覆盖),授权关卡是前端那道强确认弹窗 (设备搬家警告 + 替换警告),不需要再加一道初始化闸门。
- **影响**:facade 删掉 `is_initialized` 闸门 (export 仍保留 NotInitialized 检查);webserver 删 409 already-initialized 映射与响应;前端删 `ALREADY_INITIALIZED` 错误分支;文案改为"会替换本机配置"。

---

## 1. 背景与根因

用户从 portable 版切到 installer 版，把 portable 的 `data` 文件夹复制到 installer 目录旁，程序读不到，只能从头重配。根因是三道独立的坎：

### 坎 1 — installer 根本不看 `<exe>/data`

路径权威 `crates/uc-app-paths/src/lib.rs`:

- portable(exe 同级有 `portable.dat`,或 `UC_PORTABLE=1`)→ `resolve_portable_root` (`lib.rs:94`) 返回 `<exe>/data`。
- installer → `base_data_local_dir` (`lib.rs:142`) 走 `dirs::data_local_dir()`(Windows `%LOCALAPPDATA%`)。

installer 同级没有 `portable.dat`,永远只读 `%LOCALAPPDATA%\app.uniclipboard.desktop\`,放在安装目录旁的 `data` 无任何代码会读。

### 坎 2 — portable 的 `data/` 多嵌套一层

`crates/uc-platform/src/app_dirs.rs::get_app_dirs()` 在 base 之上再 join 一次 `app.uniclipboard.desktop`,实际布局：

```text
<exe>/data/app.uniclipboard.desktop/uniclipboard.db   ← 真正的库
<exe>/data/logs/                                       ← 日志(不在上面那层)
```

而 installer 库在 `%LOCALAPPDATA%\app.uniclipboard.desktop\uniclipboard.db`。正确手动迁移需要把 `data\app.uniclipboard.desktop\*` **合并** 进 `%LOCALAPPDATA%\app.uniclipboard.desktop\`,多一层嵌套，用户基本猜不到。

### 坎 3 — 密钥后端在两种模式下不同 (最隐蔽，导致「必须重新配对」)

`crates/uc-platform/src/secure_storage.rs`:

- portable 强制 `FileSecureStorage`,密钥写成 `…/keyring/*.bin`(兑现「绿色版不留痕」,`secure_storage.rs:318`)。
- installer 优先 OS keyring(Windows 凭据管理器),仅探测失败才降级文件后端 (`secure_storage.rs:330`)。

正常 Windows 上 installer 只读凭据管理器、不读复制过来的 `keyring/*.bin`。桥接「旧后端→新后端」的 `MigratingSecureStorage` 目前 **未被装配**(`assembly.rs` 直接拿单一后端),且其设计用途是「旧 OS keyring 位置→新 OS keyring」,不覆盖「文件→凭据管理器」。

### 可恢复 vs 不可恢复 (决定难点)

- **加密历史不会丢。** unlock 走 `derive_kek_argon2id(passphrase, keyslot.salt, kdf)`(`crates/uc-infra/src/security/space_access_adapter.rs`):KEK 是用 **passphrase + DB 内 salt 现场 Argon2 派生**,keyring 里的 KEK 只是「免输密码自动解锁」的缓存。只要 `uniclipboard.db` 在 + 记得 passphrase，重输一次密码即可解开全部历史。
- **真正带不过去的是 iroh 设备身份。** `iroh-identity:v1`(`crates/uc-infra/src/network/iroh/identity_store.rs:26`) 是随机生成、不可派生的 32 字节 Ed25519 私钥，只经 secure storage 存取。installer 读不到那个文件 → `ensure_secret_key` 生成全新 NodeId → 对所有对端是新设备 → **必须重新配对**。

> 结论：用户说的「从头重配」≈ 重设 passphrase(其实可省)+ **重新配对所有设备**(无法靠复制文件规避)。本功能的核心价值，就是把 **坎 3 的密钥后端鸿沟** 抹平，做到迁移后 **无需重新配对**。

---

## 2. 目标 / 非目标

### 2.0 定位：迁移/搬家 (已锁定)

本功能定位为 **迁移 (move)**,不是「备份到多机」:导入会把 iroh 设备身份 (NodeId) 一起带到目标机，**目标机即成为「同一台设备」**。因此：

- 保留身份 → 迁移后 **无需重新配对**(核心价值)。
- **假定源端弃用**。UI 在导出/导入处必须强提示：**「迁移完成后请勿再让源设备上线;两个相同 NodeId 同时在线会在 P2P 网络上冲突」**。
- 不把它宣传成「随时多机恢复的备份」;多机恢复带来的同身份冲突不在 v1 处理范围。

### 目标

1. 应用内一键 **导出** 当前安装的完整配置为单个加密文件 (`.ucbundle`)。
2. 在另一安装 (空白未初始化) 上一键 **导入** 该文件，完成后：
   - 剪贴板历史可解锁 (passphrase 不变)。
   - **设备身份不变 → 无需重新配对**(把 iroh 身份写进目标机当前 secure storage 后端)。
   - 设置项原样恢复。
3. 对称覆盖 portable↔installer、以及跨机器迁移;顺带成为「备份/恢复」能力。
4. 守住 VISION 红线:portable 不留痕、加密不可绕过、密钥材料不明文落盘。

### 非目标 (v1)

- 不做「installer 启动自动扫描并吞掉某个 `data` 目录」(安全顾虑 + 与 portable 设计冲突;若需要，作为 v2 在本引擎之上叠加「首启检测到旧数据→提示导入」的 UX)。
- 不做覆盖式导入 (目标机已初始化时不就地合并);已初始化必须先 factory-reset。
- 不做选择性/部分导入 (只导某些条目);v1 是整份迁移。
- 不做多 profile 同时迁移的 UI(引擎按 scope 枚举，但 UI 先针对单 profile)。

---

## 3. 核心设计决策

### 3.1「一份完整配置」包含什么

| 项 | 来源 | 是否入包 | 说明 |
|---|---|---|---|
| `uniclipboard.db` | data root | ✅ 必须 | 加密历史 + 设备/peer 元数据 (KeySlot 不在此，见下一行) |
| `vault/device_id.txt` | data root | ✅ 必须 | 业务设备 UUID |
| `vault/keyslot.json` | data root | ✅ 必须 | KeySlot(salt/kdf/wrapped master key);`JsonKeySlotStore` 根在 `vault_dir`(`assembly.rs:485`),是独立文件不在 DB |
| `vault/.setup_status` | data root | ✅ 必须 | `SetupStatus{has_completed,space_id}`,`FileSetupStatusRepository` 根在 `vault_dir`(`setup_status.rs:13`)。**这是 facade「已初始化」闸门的真相源**——漏带它则导入后机器显示未初始化 (实现期 gate 发现的遗漏，已补) |
| `settings.json` | data root | ✅ 必须 | 用户设置 |
| `iroh-identity/` 目录 | data root | ✅ 必须 | iroh 设备身份 (`iroh-identity:v1`) 实际是 `<app_data>/iroh-identity/` 下的 **0600 文件**(`space_setup.rs:306` 专用 `FileSecureStorage`,故意不进 keychain),**不在 platform.secure_storage**。因此当 **文件目录** 迁移 (实现期 gate 修正，原假设「身份在 secure storage」是错的)。不可派生，缺它=重新配对，**本功能存在的理由** |
| `kek:v1:profile:{id}` | **secure storage** | ⚠️ 可选 | 在 `platform.secure_storage`(installer=keychain / portable=`keyring/` 文件)。可由 passphrase 重新派生;入包=导入后免输密码自动解锁。见 3.3。**唯一需要 secure-storage 桥接的 secret** |
| `last_notified_update.json` / `skipped_version.json` | data root | 可选 | UI 状态，带上更顺滑 |
| `file-cache/`、`cache/spool/`、`blobs` 缓存、`logs/` | data root | ❌ 不入包 | 可重建缓存，徒增体积 |
| `.daemon-token`、`.daemon-pid` | data root | ❌ 不入包 | 进程级，目标机重新生成 |

### 3.2 包必须加密

包内含 **iroh 私钥**(可冒充设备身份)+ DB 元数据，因此 **整包必须加密落盘**,绝不明文。

- 加密算法复用项目既有 XChaCha20-Poly1305 AEAD(与 VISION 一致)。
- 包密钥 = `Argon2id(export_password, salt)`,salt 随包存。
- **(已锁定)** `export_password` 默认建议用户 **直接用 Space passphrase**(导入端只需记一个秘密),UI 默认填充/提示该选项，同时允许改用自定义口令。

### 3.3 KEK 入包 (已锁定：入包)

- **决策:KEK 入包。** 导入后可直接自动解锁，零 passphrase 重输，最顺滑。
- 因为整包已被 `export_password` 加密，入包 KEK 不额外降低安全性 (尤其当 `export_password` 就是 passphrase 时，二者等价)。
- KEK 仍是 passphrase 可重新派生的冗余项，故落地失败时可降级为「不写 KEK，导入后让用户输 passphrase 解锁」,不阻断迁移。

### 3.4 导入前置：目标机必须未初始化

判定用 `KeyMaterialStore::keyslot_exists()`(`crates/uc-infra/src/security/key_material.rs:83`)。

- 未初始化 (`keyslot_exists() == false`)→ 允许导入。
- 已初始化 → 拒绝，返回 `AlreadyInitialized`,提示用户先走 `/encryption/factory-reset`(已存在，`crates/uc-webserver/src/api/encryption.rs`) 再导入。v1 不做就地覆盖 + 回滚，避免复杂事务。

### 3.5 导入采用「暂存 + 重启时落地」(stage then apply on boot)

running daemon 持有 sqlite 句柄与 secure storage，直接热替换整库有竞态。改为：

1. daemon 收到导入请求，校验未初始化 + 解密校验通过后，把解包内容写入 data root 下的 `import-staging/`,并写 `pending-import.json` 标记。
2. daemon 返回成功 → 前端触发 daemon 重启。
3. daemon 重启时，bootstrap(`assembly.rs::wire_dependencies` 之前) 检测 `pending-import.json`:
   - 把 db / vault / **iroh-identity/ 目录** / settings 落到正式位置 (iroh 身份是文件，直接复制，无需 secure storage);
   - 把 secrets(**仅 KEK**) 写入 **当前** secure storage 后端 ← **坎 3 的桥接点**(installer 上即写进凭据管理器);KEK 可派生，失败可降级为导入后输 passphrase;
   - 删除 staging + 标记;
   - 继续正常启动 → 此时已是「已初始化 + 身份延续」状态。

此模式与项目既有的 boot 期状态文件 (`upgrade-cursor.json`、`first-sync-state.json`、`migration-state`、handover file) 一致，race 最小。

> 备选 (评审可议):导入要求未初始化，DB 尚无 space 数据，理论上可「live 写 secrets + 落 db 后重启」。但 boot 期落地更干净统一，设为首选。

### 3.6 导出需要已解锁会话

导出会把 iroh 私钥 + 加密库打成可携带文件。要求 **当前会话已 unlock**(证明操作者掌握 passphrase),避免有人对着锁定的机器把身份与数据导走。读取 secrets/db/文件本身不需要 unlock，但以「已解锁」作为授权闸门。

### 3.7 DB 一致性快照

daemon 持库时不可裸拷开着 WAL 的 `uniclipboard.db`。导出用 sqlite 在线备份 / `VACUUM INTO` 产出一致性快照文件再入包。

---

## 4. 包格式 (`.ucbundle`)

```text
┌─ 明文头(未加密,便于校验/版本协商)
│   magic        = "UCBUNDLE"
│   format_ver   = 1
│   kdf          = { algo: "argon2id", m, t, p }
│   salt         = <16B>
│   nonce        = <24B XChaCha20>
├─ 密文(XChaCha20-Poly1305, key = Argon2id(export_password, salt), AAD = 明文头)
│   └─ 解密后是一个 tar:
│       manifest.json      # schema_ver, app_version, created_at(由调用方注入),
│                          # source_mode(portable/installer), profile_id,
│                          # device_fingerprint, included[]
│       db/uniclipboard.db # 一致性快照
│       vault/device_id.txt
│       vault/keyslot.json
│       vault/.setup_status
│       iroh-identity/*    # iroh 设备身份文件(0600),当文件迁移(非 secret)
│       settings.json
│       secrets.json       # { secrets: { "kek:v1:profile:..": <b64> } } —— 仅 KEK
│       ui-state/*.json    # 可选
└─
```

- `manifest.json` 带版本 → 导入时校验兼容 (`format_ver` / `schema_ver` / keyslot `"V1"` / kek 前缀 `kek:v1:`)。
- 文件名/扩展名待定 (bikeshed):`.ucbundle` 暂定。

---

## 5. 架构落点

遵循六边形 + daemon/GUI 分离:DB 与 secure storage 属 daemon，故导出/导入主体是 **daemon 侧操作**,经 daemon HTTP 暴露;Tauri 仅负责文件对话框等 OS 交互。

### 5.1 daemon HTTP 端点 (`crates/uc-webserver/src/api/`)

新增 `config.rs`,在 `routes.rs::router_l2_plus` 注册 (需 session JWT),参照既有 `/encryption/factory-reset`、`/storage/clear-cache` 风格 (ApiEnvelope 规范、`confirmed` 闸门):

- `POST /config/export` — body `{ password, target_path }`;前置：已 unlock。daemon 产快照、读 secrets、打包加密、写到 `target_path`(daemon 有 fs 权限，与 `exportLogs` 返回 path 的模式一致),返回 `{ path }`。
- `POST /config/import` — body `{ password, source_path, confirmed }`;前置：未初始化。daemon 读文件、解密、校验、写 `import-staging/` + `pending-import.json`,返回 `{ stagedOk: true }`。
- (可选)`GET /config/import/preview` — 只解密 manifest，返回 `app_version/source_mode/created_at/fingerprint` 供 UI 二次确认。

> OpenAPI:`#[utoipa::path(...)]` 用 BARE schema 名 + `ApiEnvelope<T>` 需 `#[aliases]`(见仓库既有约定),生成物有 CI drift check。

### 5.2 应用层 facade(`crates/uc-application/src/facade/`)

新增 `config_migration/`,定义 use case:`export_config(password, target) -> Result<Path, _>`、`stage_import(password, source) -> Result<(), _>`。通过 port 调用 infra 能力，**不在 facade 写文件/加密细节**。

### 5.3 新增 ports(`uc-core`)+ infra 实现 (`uc-infra`)

- 复用既有：`SecureStoragePort`(读/写 secrets)、`KeyMaterialStore::keyslot_exists`、AppPaths。
- 新增落在 **uc-infra** 的 `config_migration` adapter:打包/解包 + AEAD，以及一个集中的 `MigratableSecretKeys` 清单 (枚举 `iroh-identity:v1` + 当前 profile 的 `kek:v1:profile:{id}`)。key 名是持久化细节，按 `scope_identifier.rs` 既定边界 **不外泄到 uc-core**(决策 Q3)。facade/port 只表达「导出/暂存导入」意图，不碰 key 名。

### 5.4 bootstrap 落地 (`crates/uc-bootstrap/src/assembly.rs`)

在解析 AppPaths 后、wire DB / secure storage 前，新增 `apply_pending_import(app_paths, secure_storage_factory)`:存在 `pending-import.json` 则落地文件 + 写 secrets，幂等、失败保留 staging 以便重试 (参照 `MigratingSecureStorage` 的「先写 primary 再删 legacy」失败语义)。

### 5.5 Tauri 命令 (`src-tauri/crates/uc-tauri/src/commands/`)

- `export_config_package()`:用 `tauri-plugin-dialog` 弹保存对话框拿 `target_path`(项目现有命令用 `tauri_plugin_opener`,保存对话框需引入 dialog 插件)→ 调 daemon `/config/export`。
- `import_config_package()`:弹打开对话框拿 `source_path` → 调 daemon `/config/import` → 成功后 `restartDaemon()`。
- 在 `specta_builder.rs::collect_commands![...]` 注册，跑 `cargo test -p uc-tauri --test specta_export` 重生成 `ipc-bindings.generated.ts`。

### 5.6 前端 (`src/components/setting/StorageSection.tsx`)

在「数据管理」分组 (约 `StorageSection.tsx:629`) 旁新增「配置备份/迁移」分组：

- 「导出配置…」按钮 → 输入导出口令 (提示可用 passphrase)→ 调命令 → 成功 toast + `revealPath`。
- 「导入配置…」按钮 → 选文件 → (可选 preview 二次确认)→ 强确认弹窗 (复用 `ClearHistoryDialog` 的 `AlertDialog` 模式)→ 进度态 → 完成后走 `DiagnosticsSettings.tsx` 同款「强制 restarting，不可关闭 → restartDaemon → restartApp」。
- 前端调用走生成的 `commands` 代理 (`src/lib/ipc.ts`) 或 daemon SDK `callEnveloped`(`src/api/daemon/client.ts`),与现有一致。

---

## 6. 详细流程

### 6.1 导出

```text
[GUI] 用户点「导出配置…」→ 输入口令 → 保存对话框选 target_path
  → Tauri export_config_package(target_path, password)
     → daemon POST /config/export { password, target_path }
        1. 校验 session 已 unlock,否则 Locked 错误
        2. keyslot_exists() 必须 true(已初始化才有东西可导)
        3. sqlite 在线备份/VACUUM INTO → db 快照
        4. 读 secrets:iroh-identity:v1(必)、kek:v1:profile:*(按 3.3)
        5. 收集 vault/、settings.json、(可选)ui-state
        6. 组 tar → Argon2id(password)派生 key → XChaCha20-Poly1305 加密 → 写 target_path
        7. 返回 { path }
  → toast 成功 + revealPath(path)
```

### 6.2 导入

```text
[GUI] 用户点「导入配置…」→ 选 source_path → (preview manifest 二次确认) → 强确认
  → Tauri import_config_package(source_path, password)
     → daemon POST /config/import { password, source_path, confirmed }
        1. keyslot_exists() 必须 false;true → AlreadyInitialized(提示先 factory-reset)
        2. 读文件 → 校验 magic/format_ver → Argon2id(password) → 解密 → 校验 AEAD tag
        3. 校验 manifest 兼容(schema_ver / keyslot V1 / kek 前缀)
        4. 解包到 import-staging/ + 写 pending-import.json
        5. 返回 { stagedOk: true }
  → 进入 restarting 态(不可关闭)→ restartDaemon()
     → daemon 重启,assembly 检测 pending-import.json:
        a. 落地 db/vault/settings 到正式位置
        b. 把 secrets 写入当前 secure storage 后端(installer→凭据管理器)← 桥接坎 3
        c. 删 staging + 标记
        d. 正常启动:已初始化 + 设备身份延续
  → restartApp() → 用户(若未入包 KEK)输入 passphrase 解锁 → 历史在、配对在
```

---

## 7. 安全考量 (VISION 对齐)

- **portable 不留痕**:导出/导入不改变 portable 的 `FileSecureStorage` 选择逻辑;portable 目标机导入后 secrets 仍只落 `keyring/*.bin`。引擎只是「读当前后端 / 写当前后端」,不跨越后端策略。
- **加密不可绕过**:包强制加密;不入包 KEK 时，导入后仍需 passphrase 解锁，符合红线。
- **iroh 私钥敏感性**:它是设备身份，泄漏=可冒充设备。包加密 + 导出需已解锁 + 日志脱敏 ([REDACTED],绝不打印 secrets/passphrase/path 敏感片段)。
- **导入即设备搬家 (见 2.0)**:导入等于把目标机变成「拥有源机身份」的设备 = 一次设备搬家。UI 必须强确认并说明：目标机将以源设备身份接入，对端会把它当作同一台设备;**迁移后请勿再让源设备上线**(同 NodeId 双在线会冲突)。这是 move 语义而非 copy。
- **遥测**:导出/导入事件名若上报需遵守「事件名上线不改名」;不得带文件名/路径/内容。

---

## 8. 边界与失败处理

- 口令错误 → 解密 AEAD 校验失败 → `InvalidPasswordOrCorrupt`(不区分，避免 oracle)。
- 包损坏/截断/版本过新 → `IncompatibleBundle { reason }`。
- 目标机已初始化 → `AlreadyInitialized`,引导 factory-reset。
- staging 落地中途崩溃 → `pending-import.json` 仍在 → 下次 boot 重试;落地是「先写新、确认后清理」,半成品不污染正式库。
- secrets 写后端失败 (如凭据管理器不可用)→ 整体失败、保留 staging,**不** 留下「库已换但身份没写」的半态 (否则会变成新身份=要重配对，违背初衷)。
- 跨平台:Windows 凭据管理器 / macOS Keychain(注意写入可能弹授权)/ Linux secret-service(KWallet 二进制 mangling #838 → 已有降级到文件后端的逻辑，导入落地需复用同一 `create_default_secure_storage_in_app_data_root` 选择结果以保持一致)。
- 大库:DB 快照 + 打包要流式/有内存上限 (uc-infra 约定);避免整库读进内存。

---

## 9. 测试计划

- 单元：包编解码 round-trip(明文头/AEAD/manifest 版本校验)、错误口令、版本不兼容、损坏截断。
- 集成 (uc-infra):secrets 读取 - 写入 round-trip;keyslot_exists 前置闸门;staging 落地幂等 + 中途失败重试。
- e2e(`tests/e2e/`,CLI 驱动):
  - portable→portable、installer→installer、**portable→installer**(核心)、installer→portable。
  - 导入后：库可解锁 (passphrase 不变)、设备 fingerprint 与源一致 (证明 iroh 身份延续 → 不重配对)、settings 一致。
  - 已初始化目标机导入被拒。
- 平台：三平台 secure storage 落地各跑一遍 (至少 CI 能跑的)。

---

## 10. 提交拆分 (atomic commits 草案)

1. `feat(core): add config-migration ports + bundle DTOs`(uc-core，只接口/DTO)
2. `feat(infra): ucbundle codec + secrets enumeration + db snapshot`(uc-infra adapter)
3. `feat(app): config_migration facade (export / stage_import)`(uc-application)
4. `feat(bootstrap): apply pending import on boot`(assembly,boot 期落地)
5. `feat(webserver): /config/export, /config/import endpoints (+openapi)`(uc-webserver，含生成物 drift)
6. `feat(tauri): export/import config commands (dialog)`(uc-tauri + specta 重生成)
7. `feat(ui): config backup/migration section in StorageSection`(前端 + i18n)
8. `test(e2e): portable↔installer config migration`(e2e)
9. `docs: config import/export user guide`(docs-site，中文，plain language 先行)

> 边界纪律：严禁一个 commit 跨 core+infra+app(见 `docs/agent/architecture-rules.md`)。

---

## 11. 决策记录 (均已锁定)

- ~~**Q1 KeySlot 存储位置**~~:**已解** — `JsonKeySlotStore`(`crates/uc-infra/src/fs/key_slot_store.rs`) 把 KeySlot 存为 `vault_dir/keyslot.json` 独立文件 (`assembly.rs:485` 根在 `vault_path`),不在 DB。包内单列 `vault/keyslot.json`。
- ~~**Q2 KEK 是否入包**~~:**已定 = 入包**(见 3.3)。
- ~~**Q3 secure storage key 清单的单一来源**~~:**已定** — 放 **uc-infra**。理由:key 名 (`iroh-identity:v1`、`kek:v1:profile:{id}`) 是持久化格式细节，`scope_identifier.rs` 已明确「属磁盘兼容不变量，不外泄到 uc-core」。新增一个 uc-infra 内的 `MigratableSecretKeys` 集中清单 / registry，导出端据此枚举，**当前 profile** 的 `kek:v1:profile:{当前 id}` + `iroh-identity:v1`。
- ~~**Q4 多 profile**~~:**已定 = 仅迁移当前 profile**;UI 不暴露多 profile。引擎按 scope 枚举但 v1 只取当前。
- ~~**Q5 包扩展名/品牌**~~:**已定** = `.ucbundle`。
- ~~**Q6 导出强制 unlocked**~~:**已定 = 是**(授权闸门，见 3.6)。
- ~~**身份语义**~~:**已定 = 迁移/搬家 (move)**,保留身份、假定源端弃用、UI 强提示勿双在线 (见 2.0 / 第 7 节)。

> 以上决策由维护者 2026-06-19 确认。后续如需「多机备份/恢复」再作为 v2 单独设计 (需处理同 NodeId 冲突)。

---

## 附录 A — 给 issue #1110 提交者的当前手动 workaround

在功能落地前，可手动迁移 **历史 + 设置**(但 **设备仍需重新配对**,因 iroh 身份在 portable 是文件、在 installer 走凭据管理器，无法靠复制带过去):

1. 关闭两端程序。
2. 把 portable 的 `…\data\app.uniclipboard.desktop\` **里面的内容** 复制并合并到 installer 的 `%LOCALAPPDATA%\app.uniclipboard.desktop\`(注意是合并这一层的内容，不是把 `data` 整个塞进去)。
3. 启动 installer 版，输入原 passphrase 解锁 → 历史与设置恢复。
4. 重新配对各设备 (此步无法规避)。

> 这正说明为什么需要本功能：坎 1/2 的路径 + 坎 3 的密钥后端，手动都难以完整跨越，尤其「不重新配对」只有应用内桥接 secure storage 才能做到。
