# Phase 94: 后端字段落地 - Context

**Gathered:** 2026-05-04
**Status:** Ready for planning

<domain>
## Phase Boundary

用户/客户端可以通过持久化 settings 与 daemon HTTP `PUT/GET /settings` 读写 `network.allow_relay_fallback` 字段；重启 daemon 后字段值通过 `uc-bootstrap` 唯一取反 helper 注入 `IrohNodeConfig.disable_relays`，使 iroh endpoint 在 bind 时按用户意愿决定是否允许 relay fallback。

**交付范围：** 后端 schema + HTTP DTO 镜像 + iroh bind 注入路径 + 防御性单测/集成测试。
**不在本 phase 范围：** 前端 UI（Phase 95）、连接通道指示器（Phase 96）、文档与 onboarding（Phase 97）、运行时热切换（整里程碑显式排除）。

</domain>

<decisions>
## Implementation Decisions

### A. 取反 helper 物理位置

- **D-A1：** `relay_policy_to_iroh_config()` 放在新建模块 `src-tauri/crates/uc-bootstrap/src/network_policy.rs`，`mod network_policy;` 在 `lib.rs` 中暴露 `pub(crate)` 即可（不对外）。Truth-table 单测以 `#[cfg(test)] mod tests` 形式紧贴函数定义放同文件。
  - 理由：模块名一眼表达"网络策略翻译"用途；未来扩"其他网络相关 settings → infra config"翻译有现成入口；单测就近放避免漏测。

### B. Settings 读取失败时的容错策略

- **D-B1：** daemon 启动期 `wired.deps.settings.load().await` 失败按错误类型分流：
  - **`NotFound`（settings.json 不存在 / 首次启动）** → 用 `Settings::default()`（即 `allow_relay_fallback: true`）继续启动，**不**写 warn（首次启动是常态）。
  - **`Parse` / `IO` / 其他错误** → 硬失败，daemon 拒绝启动并暴露错误（避免脏 settings 让 LAN-only 信任锚点撒谎）。
- **D-B2：** 实施前 planner 需要核对 `SettingsPort::load` 的当前错误返回类型 —— 若现有错误 enum 没有区分 NotFound vs Parse，需要在 `uc-core::ports::settings` 或对应 error type 里补区分（属于 Phase 94 范围内的小调整，不另开 phase）。
- **D-B3：** 启动路径打 `tracing::info!` 一行：`applying network.allow_relay_fallback={value} → disable_relays={value}`，方便 support 排障。

### C. 集成测试位置 + 范围

- **D-C1：** 测试按层切分：
  - `src-tauri/crates/uc-infra/tests/lan_only_relay_mode.rs` —— 直接构造 `IrohNodeConfig { disable_relays: true | false, .. }`，bind 后断言 `Endpoint::addr().addrs` 含/不含 `TransportAddr::Relay` 项；两组用例覆盖。
  - `uc-bootstrap/src/network_policy.rs#[cfg(test)] mod tests` —— helper truth-table 单测覆盖 `(allow=true → disable=false)` 与 `(allow=false → disable=true)` 两组。
- **D-C2：** **不**新增 uc-bootstrap 端到端 settings→bind injection 集成测试 —— 端到端链路由后续 phase 集成测试 + 手工验收覆盖（roadmap 验收标准 #1 已锁定手工流程）。

### D. `restart_required` 信号的 phase 归属

- **D-D1：** Phase 94 的 HTTP `PUT /settings` 响应 body 加 `restart_required: bool` 字段。
  - 当 patch 影响 `network` 字段（即 patch 中 `network` 段非空且至少含一个字段变更）时返回 `true`；否则 `false`。
  - 若 patch 同时影响其他字段（如 `general.theme`），`restart_required` 仍按 `network` 字段是否变化决定，与其他字段无关（其他字段本身不需重启）。
- **D-D2：** application 层 `UpdateNetworkSettings` 或等效 use case 必须返回 `restart_required: bool`（Pitfall 3 防御 —— 调用方显式承担"还没真正生效"的事实）。
- **D-D3：** OpenAPI schema 同步更新：`UpdateSettingsResponse` DTO 加 `restart_required: bool` 字段。

### Claude's Discretion

- **DTO 字段名 rust ↔ JSON：** Rust 端 `restart_required: bool`，JSON wire `restartRequired: boolean`（沿用现有 `#[serde(rename_all = "camelCase")]` 模式，无需特别决策）。
- **`UpdateNetworkSettings` use case 命名：** 是否真要新建一个独立 use case，还是在 `UpdateSettings` 里加 `restart_required` 计算逻辑 —— 由 planner 决定。
- **iroh integration test fixture：** 是否复用 `tests/iroh_presence_probe.rs:17-29` / `slice2_phase1_presence_e2e.rs:354-356` 的 loopback 双 endpoint pattern —— planner 自由发挥。
- **logging tracing target / level：** `info!` vs `debug!`，target name —— planner 决定。

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 里程碑与需求层

- `.planning/ROADMAP.md` §Phase 94 — 4 条 success criteria + 5 条 pitfall 防御
- `.planning/REQUIREMENTS.md` NETSET-01 / NETSET-02 / NETSET-03 — 字段定义、HTTP 行为、bind 注入语义
- `.planning/PROJECT.md` §当前里程碑 + §Out of Scope — 不联动遥测、不做运行时热切换
- `.context/attachments/Summary of Explore LAN version need.md` — 反向命名决策来源（如仍存）

### 研究层

- `.planning/research/SUMMARY.md` — 主因 A/B 收敛 + 综合 phase 排序
- `.planning/research/STACK.md` §1 + §2 — iroh 0.98 API（`remote_info`/`TransportAddr`）、settings `network` namespace 扩展模式
- `.planning/research/ARCHITECTURE.md` §0 + §1.1–1.4 + §3 — 五层落点 + 行号锚定
- `.planning/research/PITFALLS.md` Pitfall 1 / 2 / 3 / 6 / 8 — 反向命名 / 默认值 / 运行时热切换 / OTLP 不联动 / 测试覆盖

### 既有 crate 文档

- `src-tauri/crates/uc-core/AGENTS.md` §6 + §8 — Network/Settings 业务 vs 实现边界规则
- `src-tauri/crates/uc-infra/AGENTS.md` §4.1 + §4.2 + §11 — infra 不让 core 适配 infra；持久化格式版本化
- `src-tauri/crates/uc-application/AGENTS.md` §11.4 — `pub use facade/` 白名单边界（更新 `pub use` 列表）

### 关键代码锚点（行号已 grep 验证）

- `src-tauri/crates/uc-core/src/settings/model.rs:201-202` — `// pub network: NetworkSettings,` 占位注释
- `src-tauri/crates/uc-core/src/settings/defaults.rs:251-262` — `Settings::default()` 字段列表
- `src-tauri/crates/uc-application/src/facade/settings/models.rs:124-225` + `446-566` — view/patch + apply_settings_patch
- `src-tauri/crates/uc-application/src/facade/settings/mod.rs:5-12` — `pub use` 白名单
- `src-tauri/crates/uc-daemon-contract/src/api/dto/settings.rs:188-208` — `FileSyncSettingsDto` 模式 + `SettingsDto` 顶层
- `src-tauri/crates/uc-webserver/src/api/settings.rs:14-218` — DTO ↔ View 映射 + `UpdateSettingsResponse`
- `src-tauri/crates/uc-webserver/src/api/openapi.rs:29-138` — OpenAPI 注册列表
- `src-tauri/crates/uc-infra/src/settings/migration.rs:36-43` — `migrations` vec 当前为空（确认 Phase 94 不动 migration）
- `src-tauri/crates/uc-infra/src/network/iroh/node.rs:153-162` — `IrohNodeConfig` 字段
- `src-tauri/crates/uc-infra/src/network/iroh/node.rs:368-411` — `bind` 时 `RelayMode` 决策路径
- `src-tauri/crates/uc-bootstrap/src/builders.rs:178` — `IrohNodeConfig::default()` 调用点 #1（GUI runtime）
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs:280` — `IrohNodeConfig::default()` 调用点 #2（CLI/daemon runtime）
- `src-tauri/crates/uc-bootstrap/src/space_setup.rs:48` + `:208-228` — `IrohNodeConfig` re-export + `build_space_setup_assembly` 入参

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- **`#[serde(default)]` 派生模式：** 现有 `Settings` 子结构（`general` / `sync` / `security` / `pairing` / `file_sync`）全部使用此模式 —— 新增 `network` 直接复用，旧 settings.json 缺 `network` 段反序列化时 `serde` 自动调 `Default`。
- **手写 `impl Default` 模式：** `uc-core/src/settings/defaults.rs` 现有所有子结构都是手写 `impl Default`（不是 `#[derive(Default)]`）—— `NetworkSettings` 跟随这个模式。
- **`SettingsView` / `SettingsPatch` 镜像模式：** `FileSyncSettingsView` / `FileSyncSettingsPatch`（`models.rs:124, 208`）是最近的同形参考；`apply_settings_patch` 末尾追加一段即可。
- **DTO ↔ View 映射 helper：** `settings_view_to_dto`（`settings.rs:162-218`）+ `settings_patch_from_dto`（`settings.rs:92-160`）—— 末尾追加 `network` 段处理，沿用 camelCase rename 模式。
- **iroh test fixture：** `uc-infra/tests/iroh_presence_probe.rs:17-29` 已有 loopback bind helper；`uc-bootstrap/tests/slice*_*.rs` 三个集成测试已使用 `IrohNodeConfig { disable_relays: true, .. }` 显式禁 relay 跑 loopback —— 新增 `lan_only_relay_mode.rs` 直接复用同模式。
- **`OnceCell` / `tokio::sync::OnceCell`：** Rust 标准实现已在 lockfile（间接通过 once_cell crate）—— `IrohNodeBuilder::bind` 加 OnceCell 守护"只能跑一次"无新依赖。

### Established Patterns

- **HTTP `/settings` patch 语义：** `apply_settings_patch` 是"only Some fields update"模式 —— 旧客户端不带 `network` 段 PUT 自动 = no-op，不抹掉已存在 `network` 字段（满足 NETSET-02 success criterion #2）。
- **DTO `serde(rename_all = "camelCase")`：** Rust `restart_required` ↔ JSON `restartRequired` 自动转换；`SettingsDto` 整族都用此模式。
- **`pub use` 白名单：** `uc-application/src/facade/settings/mod.rs:5-12` 是对外类型白名单 —— 新增类型必须显式 `pub use`，不允许下游直接从 `models.rs` import（防 §11.4 历史欠账重发）。
- **`schema_version = 1` + 空 migrations vec：** Phase 94 加字段但**不**升 version；`SettingsMigrator::new()` `migrations: vec![]` 保持空（已 grep 确认）。

### Integration Points

- **新增 helper 入口：** `uc-bootstrap/src/network_policy.rs::relay_policy_to_iroh_config(allow_relay_fallback: bool, rendezvous_base_url: Option<String>) -> IrohNodeConfig` —— `pub(crate)` 即可，不对外暴露。
- **`builders.rs:178` 改造：** 改造前先 `wired.deps.settings.load().await`（按 D-B1 错误类型分流），再调 `relay_policy_to_iroh_config(settings.network.allow_relay_fallback, None)` 喂给 `build_space_setup_assembly`。
- **`non_gui_runtime.rs:280` 改造：** 同 builders.rs 模式，两处都需要改。
- **`build_space_setup_assembly` 签名：** **不动**。仍接 `IrohNodeConfig`；翻译职责在调用方完成（保持装配体只装配不决策）。
- **`IrohNodeBuilder::bind` 加 OnceCell 守护：** 保证单进程只能 bind 一次，防止运行时热切换的诱惑（Pitfall 3 结构性防御）。
- **`UpdateSettingsResponse` 加 `restart_required: bool`：** wire 契约改动 —— OpenAPI schema 同步更新；前端 client（`src/api/daemon/settings.ts`）需要在 Phase 95 同步消费（Phase 94 仅暴露 wire）。
- **`apply_settings_patch` 末尾追加 `network` 处理：** 与 `file_sync` 同形 pattern。
- **`uc-application/src/facade/settings/mod.rs` `pub use` 列表：** 加 `NetworkSettingsView, NetworkSettingsPatch`。

</code_context>

<specifics>
## Specific Ideas

- **唯一取反点 grep 守护：** PR review 时 `rg 'disable_relays|allow_relay_fallback'` 确认全工程除 `uc-bootstrap/src/network_policy.rs`（取反点）+ `uc-infra/src/network/iroh/node.rs`（infra 字段定义）+ 测试文件外，其他位置只以**原语义**（不取反）流动。
- **`NetworkSettings` 字段顺序：** 当前只有 `allow_relay_fallback` 一个字段，预留扩展（如未来 `rendezvous_base_url_override`）；不强制添加 `#[non_exhaustive]`，但保留这个可能性给后续 phase 决策。
- **`tracing::info!` 启动日志格式：** `applying network.allow_relay_fallback={value} → disable_relays={value}` 字段名照抄；不在 OTLP 上加 attrs（避免与 Pitfall 6 OTLP 不联动原则擦边）。
- **`Default` 注释三行警示：** `impl Default for NetworkSettings` 上方注释明确：
  ```
  // 默认 true = 允许 fallback。
  // 改成 false 会让所有跨网段老用户突然离线，属于 breaking change。
  // 修改默认值前请先 grep `LAN-only Mode` 文档与 changelog。
  ```

</specifics>

<deferred>
## Deferred Ideas

- **PR 模板 checkbox "[ ] 我没有尝试在运行时重建 iroh endpoint"** — Pitfall 3 提到的工程化防御。Phase 94 不动 PR 模板（结构守护已由 `OnceCell` + grep 替代）；如里程碑末期发现仍有 review 漏，留给 Phase 97（`docs/terminology.md` + reviewer checklist）一起加。
- **`UpdateNetworkSettings` 独立 use case 拆分** — Pitfall 3 提到此 use case 名。是否拆出独立 use case（vs. `UpdateSettings` 内分支判断）由 planner 决定；当前 `SettingsFacade::update` 是泛 patch，可不拆。
- **`IrohNode::endpoint()` 访问器方式（Open Question 1）** — Phase 96 连接通道指示器需要 `Endpoint` 句柄；本 phase 不暴露 `pub fn endpoint()`，留给 Phase 96 决策（建议 `IrohNodeBuilder::spawn()` 顺带返回 `ConnectionChannelPort` 句柄，与 `install_*` 模式一致）。
- **runtime 热切换 LAN-only Mode** — 整里程碑显式排除（PROJECT.md §Out of Scope），需独立 phase + 重建 endpoint + ALPN handler 重挂；本 phase 主动用 `OnceCell` 阻断这条路。
- **OTLP `connection_path` 标签** — Future Requirements D4，v0.7.x 再做。

### Reviewed Todos (not folded)

无 — `gsd-tools todo match-phase 94` 返回 0 个匹配（`todo_count=7` 但都不属于本 phase 范围）。

</deferred>

---

*Phase: 94-后端字段落地*
*Context gathered: 2026-05-04*
