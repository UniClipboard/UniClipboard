# Phase 94: 后端字段落地 - 模式映射

**Mapped:** 2026-05-04
**Files analyzed:** 11 (8 修改 + 3 新建)
**Analogs found:** 11 / 11 (full coverage,均为已验证行号)

---

## 文件分类

| 文件 | 角色 | 数据流 | 最近 analog | 匹配度 |
|------|------|--------|-------------|--------|
| `src-tauri/crates/uc-core/src/settings/model.rs` | model (值对象 + Settings 字段) | static config | 同文件 `FileSyncSettings` (`model.rs:166-174`) + `Settings.file_sync` (`model.rs:200`) | exact (同文件同形态) |
| `src-tauri/crates/uc-core/src/settings/defaults.rs` | model (Default impl) | static config | 同文件 `impl Default for FileSyncSettings` (`defaults.rs:205-225`) + `Default for Settings` (`defaults.rs:251-263`) | exact |
| `src-tauri/crates/uc-application/src/facade/settings/models.rs` | application view/patch + apply_patch | request-response (write-merge) | 同文件 `FileSyncSettingsView` (`models.rs:124-131`) + `FileSyncSettingsPatch` (`models.rs:208-215`) + `apply_settings_patch` 末段 (`models.rs:544-563`) | exact |
| `src-tauri/crates/uc-application/src/facade/settings/mod.rs` | facade re-export 白名单 | export-only | 现有 11 行 `pub use` (`mod.rs:5-12`) | exact |
| `src-tauri/crates/uc-daemon-contract/src/api/dto/settings.rs` | DTO (wire schema) | request-response | 同文件 `FileSyncSettingsDto` (`settings.rs:186-195`) + `FileSyncSettingsPatchDto` (`settings.rs:288-297`) + `From<core::FileSyncSettings>` (`settings.rs:458-468`) + `SettingsDto` 顶层 (`settings.rs:197-208`) | exact |
| `src-tauri/crates/uc-webserver/src/api/settings.rs` | 路由 handler + DTO ↔ View 映射 | request-response | 同文件 `settings_patch_from_dto` 末段 (`settings.rs:149-158`) + `settings_view_to_dto` 末段 (`settings.rs:209-216`) + `UpdateSettingsResponse` 当前定义在 dto/settings.rs (`uc-daemon-contract/.../settings.rs:17-23`) | exact |
| `src-tauri/crates/uc-webserver/src/api/openapi.rs` | OpenAPI registration | declarative config | 同文件现有 `dto::settings::{...}` import (`openapi.rs:28-33`) + components.schemas list (`openapi.rs:107-141`) | exact |
| `src-tauri/crates/uc-bootstrap/src/builders.rs` | bootstrap 装配点 #1 (GUI/daemon) | startup wiring | 同文件 `IrohNodeConfig::default()` 直传 (`builders.rs:178`) | exact (改造点) |
| `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` | bootstrap 装配点 #2 (CLI) | startup wiring | 同文件 `IrohNodeConfig::default()` 直传 (`non_gui_runtime.rs:280`) | exact (改造点) |
| **(NEW)** `src-tauri/crates/uc-bootstrap/src/network_policy.rs` | helper module (取反翻译) | pure function | 无直接 analog;最近的"翻译/装配 helper"模式 = `space_setup::build_space_setup_assembly` (`space_setup.rs:208-228`) | partial-match (新模块) |
| **(NEW)** `src-tauri/crates/uc-infra/tests/lan_only_relay_mode.rs` | integration test (iroh bind 时行为) | request-response | `uc-infra/tests/iroh_presence_probe.rs:17-29` (loopback bind helper) + `slice1_handshake_e2e.rs:342-350` / `slice2_phase1_presence_e2e.rs:352-360` (`IrohNodeConfig { disable_relays: true, .. }` 显式构造) | exact |

---

## 模式分配

### 1. `src-tauri/crates/uc-core/src/settings/model.rs` (model, static config)

**Analog:** 同文件 `FileSyncSettings` 子结构 (lines 166-174) + `Settings.file_sync` 挂载 (line 200)

**导入模式 (lines 1-7,无变化,新增字段不需要新 import):**
```rust
use std::collections::HashMap;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
```

**子结构定义模式 (`FileSyncSettings` lines 166-174,新增 `NetworkSettings` 镜像):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSyncSettings {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
}
```

**字段 default helper 模式 (`default_telemetry_enabled` lines 29-31,关键样板,bool 字段必须显式提供 fn):**
```rust
fn default_telemetry_enabled() -> bool {
    true
}
```

**Settings 顶层挂载模式 (lines 199-202,占位行已就位):**
```rust
    #[serde(default)]
    pub file_sync: FileSyncSettings,
    // #[serde(default)]
    // pub network: NetworkSettings,    ← 取消注释 + 真实定义在新增 struct 之后
```

**改造动作:**
1. 在 `FileSyncSettings` 之后(line 175 附近)新增 `pub struct NetworkSettings { #[serde(default = "default_allow_relay_fallback")] pub allow_relay_fallback: bool }`,加 `default_allow_relay_fallback() -> bool { true }` helper
2. 取消 `model.rs:201-202` 的注释占位,挂入 `Settings`

**Pitfall 防御 (来自 PITFALLS.md Pitfall 2):**
- `NetworkSettings` **禁止** `#[derive(Default)]`(`bool::default() == false` 极度危险);Default impl 写在 `defaults.rs`(见下文 §2)
- `default_allow_relay_fallback()` 字面量 `true` 上方加注释:"默认 true = 允许 fallback。改成 false 会让所有跨网段老用户突然离线,属于 breaking change。修改默认值前请先 grep `LAN-only Mode` 文档与 changelog。"
- **不**升 `CURRENT_SCHEMA_VERSION`(`#[serde(default)]` 已覆盖向后兼容)

---

### 2. `src-tauri/crates/uc-core/src/settings/defaults.rs` (model, static config)

**Analog:** 同文件 `impl Default for FileSyncSettings` (lines 205-225) + `Default for Settings` 末段 (lines 251-262)

**手写 Default impl 模式 (`FileSyncSettings` lines 205-225,所有子结构均为手写,不用 derive):**
```rust
impl Default for FileSyncSettings {
    /// Returns default `FileSyncSettings` enabling file sync with sensible limits.
    ///
    /// Defaults:
    /// - `file_sync_enabled`: true
    /// - `small_file_threshold`: 10 MB (inline transfer threshold)
    /// ...
    fn default() -> Self {
        Self {
            file_sync_enabled: true,
            small_file_threshold: 10 * 1024 * 1024, // 10 MB
            max_file_size: 5 * 1024 * 1024 * 1024,  // 5 GB
            file_cache_quota_per_device: 500 * 1024 * 1024, // 500 MB
            file_retention_hours: 24,
            file_auto_cleanup: true,
        }
    }
}
```

**Settings 默认装配模式 (lines 251-262,字段顺序与 model.rs:177-203 严格一致):**
```rust
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            general: GeneralSettings::default(),
            sync: SyncSettings::default(),
            retention_policy: RetentionPolicy::default(),
            security: SecuritySettings::default(),
            pairing: PairingSettings::default(),
            keyboard_shortcuts: HashMap::new(),
            file_sync: FileSyncSettings::default(),
            // ← 新增 network: NetworkSettings::default(),
        }
    }
}
```

**改造动作:**
1. 在 `impl Default for FileSyncSettings` 之后(line 226 附近)新增 `impl Default for NetworkSettings { fn default() -> Self { Self { allow_relay_fallback: true } } }`,正上方加 §1 提到的三行警示注释
2. `Default for Settings`(line 251-262)在 `file_sync: ...` 之后追加一行 `network: NetworkSettings::default(),`

---

### 3. `src-tauri/crates/uc-application/src/facade/settings/models.rs` (application view/patch + apply_patch)

**Analog:** 同文件 `FileSyncSettingsView` (lines 124-131) + `FileSyncSettingsPatch` (lines 208-215) + `apply_settings_patch` 末段 (lines 544-563)

**View 镜像模式 (lines 124-131):**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSyncSettingsView {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
}
```

**Patch 镜像模式 (lines 207-215,所有字段是 `Option<T>`,默认是 `Default`):**
```rust
#[derive(Debug, Clone, Default)]
pub struct FileSyncSettingsPatch {
    pub file_sync_enabled: Option<bool>,
    pub small_file_threshold: Option<u64>,
    pub max_file_size: Option<u64>,
    pub file_cache_quota_per_device: Option<u64>,
    pub file_retention_hours: Option<u32>,
    pub file_auto_cleanup: Option<bool>,
}
```

**SettingsView 顶层挂载模式 (lines 133-143):**
```rust
#[derive(Debug, Clone)]
pub struct SettingsView {
    pub schema_version: u32,
    pub general: GeneralSettingsView,
    pub sync: SyncSettingsView,
    // ...
    pub file_sync: FileSyncSettingsView,
    // ← 新增 pub network: NetworkSettingsView,
}
```

**SettingsPatch 顶层挂载模式 (lines 217-226):**
```rust
#[derive(Debug, Clone, Default)]
pub struct SettingsPatch {
    pub general: Option<GeneralSettingsPatch>,
    pub sync: Option<SyncSettingsPatch>,
    // ...
    pub file_sync: Option<FileSyncSettingsPatch>,
    // ← 新增 pub network: Option<NetworkSettingsPatch>,
}
```

**core → View 映射模式 (`From<core::Settings> for SettingsView` lines 386-444,末段):**
```rust
            file_sync: FileSyncSettingsView {
                file_sync_enabled: value.file_sync.file_sync_enabled,
                small_file_threshold: value.file_sync.small_file_threshold,
                // ...
                file_auto_cleanup: value.file_sync.file_auto_cleanup,
            },
            // ← 新增 network: NetworkSettingsView { allow_relay_fallback: value.network.allow_relay_fallback },
        }
    }
}
```

**apply_settings_patch 末段模式 (lines 544-563,关键 — only Some fields update):**
```rust
    if let Some(file_sync) = patch.file_sync {
        if let Some(v) = file_sync.file_sync_enabled {
            existing.file_sync.file_sync_enabled = v;
        }
        if let Some(v) = file_sync.small_file_threshold {
            existing.file_sync.small_file_threshold = v;
        }
        // ...
        if let Some(v) = file_sync.file_auto_cleanup {
            existing.file_sync.file_auto_cleanup = v;
        }
    }

    // ← 末段新增:
    // if let Some(network) = patch.network {
    //     if let Some(v) = network.allow_relay_fallback {
    //         existing.network.allow_relay_fallback = v;
    //     }
    // }

    existing
}
```

**改造动作:**
1. 在 `FileSyncSettingsView` 之后(line 132 附近)新增 `NetworkSettingsView { pub allow_relay_fallback: bool }`,trait derive 与 `FileSyncSettingsView` 相同(`Debug, Clone, PartialEq, Eq`)
2. 在 `FileSyncSettingsPatch` 之后(line 216 附近)新增 `NetworkSettingsPatch { pub allow_relay_fallback: Option<bool> }`,trait derive 与 `FileSyncSettingsPatch` 相同(`Debug, Clone, Default`)
3. `SettingsView`(line 142 之后)、`SettingsPatch`(line 225 之后)各加 `network` 字段
4. `From<core::Settings> for SettingsView`(line 442 之后)末段补 `network: NetworkSettingsView { ... }`
5. `apply_settings_patch`(line 563 之后)末段补 `if let Some(network) = patch.network { ... }`

**Pitfall 防御:**
- `NetworkSettingsPatch` 必须 `#[derive(Default)]`(empty patch = 全部 None,符合 "patch only Some fields update" 语义,旧客户端不带 `network` 段不会抹掉已有字段)

---

### 4. `src-tauri/crates/uc-application/src/facade/settings/mod.rs` (facade re-export 白名单)

**Analog:** 同文件 lines 5-12 现有 `pub use` 列表(11 个 view/patch 类型)

**白名单模式 (lines 1-13,严格 alphabetic 顺序):**
```rust
mod facade;
mod models;

pub use facade::{SettingsFacade, SettingsFacadeError};
pub use models::{
    ContentTypesPatch, ContentTypesView, FileSyncSettingsPatch, FileSyncSettingsView,
    GeneralSettingsPatch, GeneralSettingsView, PairingSettingsPatch, PairingSettingsView,
    RetentionPolicyPatch, RetentionPolicyView, RetentionRulePatchValue, RetentionRuleView,
    RuleEvaluationView, SecuritySettingsPatch, SecuritySettingsView, SettingsPatch, SettingsView,
    ShortcutKeyView, SyncFrequencyView, SyncSettingsPatch, SyncSettingsView, ThemeView,
    UpdateChannelView,
};
```

**改造动作:**
- 在第 8 行(`PairingSettingsPatch, PairingSettingsView,` 之后)插入 `NetworkSettingsPatch, NetworkSettingsView,`(保持 alphabetic 顺序;`Network` 排在 `Pairing` 之前,所以实际位置在第 8 行的 `Pairing*` **之前**;按现行排序应在第 7 行的 `GeneralSettingsView,` 之后)

**Pitfall 防御 (来自 AGENTS.md §11.4):**
- 严禁 `lib.rs` 或外部 crate 直接 `use uc_application::facade::settings::models::NetworkSettingsView`;必须通过 `pub use` 白名单暴露
- 白名单是 §11.4.7 的"新增类型必须显式 pub use"红线,反模式直接 reject

---

### 5. `src-tauri/crates/uc-daemon-contract/src/api/dto/settings.rs` (DTO + From mappings)

**Analog:** 同文件 `FileSyncSettingsDto` (lines 186-195) + `FileSyncSettingsPatchDto` (lines 288-297) + `From<core::FileSyncSettings>` (lines 458-468) + `SettingsDto.file_sync` (line 207) + `UpdateSettingsResponse` 当前定义 (lines 17-23)

**DTO 镜像模式 (lines 186-195,加 `ToSchema` + camelCase rename):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileSyncSettingsDto {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
}
```

**PatchDto 镜像模式 (lines 288-297,字段全 `Option<T>`):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileSyncSettingsPatchDto {
    pub file_sync_enabled: Option<bool>,
    pub small_file_threshold: Option<u64>,
    pub max_file_size: Option<u64>,
    pub file_cache_quota_per_device: Option<u64>,
    pub file_retention_hours: Option<u32>,
    pub file_auto_cleanup: Option<bool>,
}
```

**SettingsDto 顶层挂载模式 (lines 197-208):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub schema_version: u32,
    pub general: GeneralSettingsDto,
    pub sync: SyncSettingsDto,
    // ...
    pub file_sync: FileSyncSettingsDto,
    // ← 新增 pub network: NetworkSettingsDto,
}
```

**SettingsPatchDto 顶层挂载模式 (lines 304-314):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatchDto {
    pub general: Option<GeneralSettingsPatchDto>,
    pub sync: Option<SyncSettingsPatchDto>,
    // ...
    pub file_sync: Option<FileSyncSettingsPatchDto>,
    // ← 新增 pub network: Option<NetworkSettingsPatchDto>,
}
```

**core → DTO 映射模式 (`From<core::FileSyncSettings>` lines 458-468):**
```rust
impl From<core::FileSyncSettings> for FileSyncSettingsDto {
    fn from(value: core::FileSyncSettings) -> Self {
        Self {
            file_sync_enabled: value.file_sync_enabled,
            small_file_threshold: value.small_file_threshold,
            // ...
            file_auto_cleanup: value.file_auto_cleanup,
        }
    }
}
```

**SettingsDto.file_sync 顶层映射模式 (`From<core::Settings>` lines 536-553):**
```rust
impl From<core::Settings> for SettingsDto {
    fn from(value: core::Settings) -> Self {
        Self {
            schema_version: value.schema_version,
            // ...
            file_sync: value.file_sync.into(),
            // ← 新增 network: value.network.into(),
        }
    }
}
```

**UpdateSettingsResponse 模式 (lines 17-23,需扩展):**
```rust
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsResponse {
    pub success: bool,
    pub data: SettingsDto,
    pub ts: i64,
    // ← 新增 pub restart_required: bool,  (D-D1/D-D3)
}
```

**改造动作:**
1. 在 `FileSyncSettingsDto` 之后(line 196 附近)新增 `NetworkSettingsDto { allow_relay_fallback: bool }`,trait derive 与 `FileSyncSettingsDto` 完全相同
2. 在 `FileSyncSettingsPatchDto` 之后(line 298 附近)新增 `NetworkSettingsPatchDto { allow_relay_fallback: Option<bool> }`
3. `SettingsDto`(line 207 之后)、`SettingsPatchDto`(line 313 之后)各加 `network` 字段
4. 新增 `impl From<core::NetworkSettings> for NetworkSettingsDto`(在 `impl From<core::FileSyncSettings>` 之后,line 469 附近)
5. `From<core::Settings> for SettingsDto`(line 550 之后)末段补 `network: value.network.into(),`
6. `UpdateSettingsResponse`(line 22 之后)新增 `pub restart_required: bool,`(camelCase wire = `restartRequired`)

**Pitfall 防御 (来自 PITFALLS.md Pitfall 1):**
- DTO 字段名 = core 字段名 = `allow_relay_fallback`(只通过 `serde(rename_all = "camelCase")` 转 wire);**禁止**在 DTO 层重命名为 `lan_only` 或类似镜像名

---

### 6. `src-tauri/crates/uc-webserver/src/api/settings.rs` (handler + DTO ↔ View 映射)

**Analog:** 同文件 `settings_patch_from_dto` 末段 (lines 149-158) + `settings_view_to_dto` 末段 (lines 209-216) + `update_settings_handler` (lines 69-85)

**Patch from DTO 末段模式 (lines 149-158):**
```rust
        file_sync: patch
            .file_sync
            .map(|file_sync| app_settings::FileSyncSettingsPatch {
                file_sync_enabled: file_sync.file_sync_enabled,
                small_file_threshold: file_sync.small_file_threshold,
                max_file_size: file_sync.max_file_size,
                file_cache_quota_per_device: file_sync.file_cache_quota_per_device,
                file_retention_hours: file_sync.file_retention_hours,
                file_auto_cleanup: file_sync.file_auto_cleanup,
            }),
        // ← 末段新增:
        // network: patch
        //     .network
        //     .map(|n| app_settings::NetworkSettingsPatch {
        //         allow_relay_fallback: n.allow_relay_fallback,
        //     }),
    }
}
```

**View to DTO 末段模式 (lines 209-216):**
```rust
        file_sync: FileSyncSettingsDto {
            file_sync_enabled: value.file_sync.file_sync_enabled,
            small_file_threshold: value.file_sync.small_file_threshold,
            max_file_size: value.file_sync.max_file_size,
            file_cache_quota_per_device: value.file_sync.file_cache_quota_per_device,
            file_retention_hours: value.file_sync.file_retention_hours,
            file_auto_cleanup: value.file_sync.file_auto_cleanup,
        },
        // ← 末段新增:
        // network: NetworkSettingsDto {
        //     allow_relay_fallback: value.network.allow_relay_fallback,
        // },
```

**update_settings_handler 模式 (lines 69-85,需扩展返回 restart_required):**
```rust
async fn update_settings_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SettingsPatchDto>,
) -> Result<Json<UpdateSettingsResponse>, ApiError> {
    let app = state.app_facade_or_error()?;
    let updated = app
        .settings
        .update(settings_patch_from_dto(payload))
        .await
        .map_err(settings_error_to_api)?;

    Ok(Json(UpdateSettingsResponse {
        success: true,
        data: settings_view_to_dto(updated),
        ts: chrono::Utc::now().timestamp_millis(),
        // ← 新增 restart_required: <bool from facade or computed here>,
    }))
}
```

**改造动作:**
1. `settings_patch_from_dto`(line 158 之后)末段补 `network` 段
2. `settings_view_to_dto`(line 216 之后)末段补 `network` 段
3. `update_settings_handler`(lines 80-84)的 response 构造加 `restart_required: bool` 字段填充
4. `restart_required` 计算逻辑:patch 中 `network` 段非空且至少含一个字段变更(D-D1)。Planner 决策:在 webserver handler 内联计算 vs application 层提供 `(SettingsView, bool)` 返回(D-D2)
5. import 列表(lines 14-20)新增 `NetworkSettingsDto, NetworkSettingsPatchDto`

**Pitfall 防御 (来自 PITFALLS.md Pitfall 1):**
- handler 里读出 `patch.network` 仅做"是否存在/字段是否变更"判断,不读取布尔值本身;**禁止**在此处对 `allow_relay_fallback` 做 `!` 运算(取反唯一一处在 §10 `network_policy.rs`)

---

### 7. `src-tauri/crates/uc-webserver/src/api/openapi.rs` (OpenAPI registration)

**Analog:** 同文件现有 `dto::settings::{...}` import (lines 28-33) + `components.schemas` list (lines 107-141)

**DTO 列表 import 模式 (lines 28-33):**
```rust
use crate::api::dto::settings::{
    ContentTypesDto, FileSyncSettingsDto, GeneralSettingsDto, GetSettingsResponse,
    PairingSettingsDto, RetentionPolicyDto, RetentionRuleDto, RuleEvaluationDto,
    SecuritySettingsDto, SettingsDto, ShortcutKeyDto, SyncFrequencyDto, SyncSettingsDto, ThemeDto,
    UpdateChannelDto, UpdateSettingsResponse,
};
```

**components.schemas 注册模式 (lines 122-141,Settings DTO 区段):**
```rust
            // Common
            ContentTypesDto,
            ApiErrorResponse,
            // ...
            GetSettingsResponse,
            UpdateSettingsResponse,
            SettingsDto,
            GeneralSettingsDto,
            SyncSettingsDto,
            SyncFrequencyDto,
            RetentionPolicyDto,
            RetentionRuleDto,
            RuleEvaluationDto,
            SecuritySettingsDto,
            PairingSettingsDto,
            FileSyncSettingsDto,
            // ← 新增 NetworkSettingsDto,
            ShortcutKeyDto,
            ThemeDto,
            UpdateChannelDto,
            // ...
```

**改造动作:**
1. import 列表(lines 28-33)新增 `NetworkSettingsDto`
2. `components.schemas`(line 138 之后,在 `FileSyncSettingsDto,` 之后)插入 `NetworkSettingsDto,`
3. `UpdateSettingsResponse` 已经在列表(line 128),无需重新加,但其字段变更会自动反映到 OpenAPI(D-D3)

---

### 8. `src-tauri/crates/uc-bootstrap/src/builders.rs` (bootstrap 装配点 #1)

**Analog:** 同文件 `IrohNodeConfig::default()` 直传 (line 178),改造为读 settings → 调 helper

**当前模式 (line 178):**
```rust
    let space_setup_assembly = build_space_setup_assembly(&wired, IrohNodeConfig::default())
        .await
        .map_err(|e| anyhow::anyhow!("Slice 1+ assembly build failed: {e}"))?;
```

**已就位的 settings 访问句柄 (来自 `wired.deps.settings: Arc<dyn SettingsPort>`,见 `assembly.rs:861`):**
```rust
// wired.deps.settings 已就位(见 assembly.rs:861 的 settings_repo 装配)
// 类型:Arc<dyn SettingsPort>,接口:async fn load(&self) -> anyhow::Result<Settings>
```

**改造模式 (按 D-B1 分流策略,Pitfall 1 + Pitfall 2 防御):**
```rust
    // 启动期读 settings:NotFound → default,其他错误 → 硬失败
    let settings = match wired.deps.settings.load().await {
        Ok(s) => s,
        Err(err) if /* 错误类型为 NotFound */ => {
            // 首次启动是常态,不打 warn
            uc_core::settings::model::Settings::default()
        }
        Err(err) => {
            // Parse / IO / 其他错误硬失败
            return Err(anyhow::anyhow!("settings load failed: {err}"));
        }
    };

    let iroh_config = crate::network_policy::relay_policy_to_iroh_config(
        settings.network.allow_relay_fallback,
        None, // rendezvous_base_url override (production 不用)
    );

    let space_setup_assembly = build_space_setup_assembly(&wired, iroh_config)
        .await
        .map_err(|e| anyhow::anyhow!("Slice 1+ assembly build failed: {e}"))?;
```

**改造动作:**
1. 在 line 178 之前插入 `wired.deps.settings.load().await` + 错误分流(D-B1)
2. 调 `crate::network_policy::relay_policy_to_iroh_config(settings.network.allow_relay_fallback, None)` 替换原来的 `IrohNodeConfig::default()`
3. `tracing::info!` 启动日志:`applying network.allow_relay_fallback={value} → disable_relays={value}`(D-B3)
4. import:加 `use crate::network_policy::relay_policy_to_iroh_config;`(若新模块定义为 `pub(crate)`)

**Pitfall 防御 (来自 PITFALLS.md Pitfall 3):**
- `IrohNodeBuilder::bind` 加 `OnceCell` 守护 → bind 一次后 attempt 第二次 panic,从结构上阻断"运行时热切换"诱惑(在 `uc-infra/src/network/iroh/node.rs` 内部)
- 本文件不写"运行时切换 settings → rebuild endpoint"的逻辑,仅启动时读一次

**注意 D-B2 决策:** Planner 实施前需要核对 `SettingsPort::load` 当前错误返回类型(目前是 `anyhow::Result<Settings>`,见 `uc-core/src/ports/settings.rs:7`,**未区分 NotFound vs Parse**)。`FileSettingsRepository::load` 内部已经做了 NotFound 兜底(`repository.rs:166-168` 直接返回 `Settings::default()`),所以**实际发生 NotFound 时不会到分流分支**;Parse / IO 错误才会冒泡。这意味着 D-B1 的"分流"在当前代码上等价于"任何 load 失败即硬失败"(NotFound 已被吃掉)。Planner 需决定:(a) 接受现状(NotFound 在 infra 层兜底,分流退化为单分支);(b) 调整 `FileSettingsRepository::load` 不兜底 + port 错误类型加区分(属本 phase 范围内的小调整,见 D-B2)。

---

### 9. `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` (bootstrap 装配点 #2)

**Analog:** 同文件 `IrohNodeConfig::default()` 直传 (line 280),改造模式与 §8 完全一致

**当前模式 (line 280):**
```rust
    let assembly = build_space_setup_assembly(&wired, IrohNodeConfig::default())
        .await
        .map_err(|err| anyhow::anyhow!("failed to bind iroh endpoint: {err}"))?;
```

**改造动作:**
- 与 §8 完全相同(读 settings → 错误分流 → 调 helper → 喂给 `build_space_setup_assembly`)
- 此文件第 278 行已经 `let (config, wired) = crate::builders::build_slice1_cli_context(log_profile)?;` 拿到 `wired`,所以 `wired.deps.settings.load().await` 在第 280 行之前直接可用

**Pitfall 防御 (来自 PITFALLS.md Pitfall 1):**
- 唯一取反点 = `network_policy.rs::relay_policy_to_iroh_config()`;此文件不写 `!settings.network.allow_relay_fallback` 类的本地取反

---

### 10. **(NEW)** `src-tauri/crates/uc-bootstrap/src/network_policy.rs` (helper module,核心新增)

**Analog:** 无直接 analog;最近的"翻译/装配 helper"模式 = `space_setup::build_space_setup_assembly` (`space_setup.rs:208-228`,接 settings → 装配 infra cfg)

**模块定义模式 (基于 `IrohNodeConfig` 已有 pub re-export `space_setup.rs:48`):**
```rust
//! 单一翻译点:`network.allow_relay_fallback`(业务正向语义)→
//! `IrohNodeConfig.disable_relays`(infra 反向语义)。
//!
//! 全工程除本模块 + `uc-infra/src/network/iroh/node.rs` 字段定义 + 测试文件
//! 外,严禁在其他位置出现 `disable_relays = !allow_relay_fallback` 类的取反
//! (Pitfall 1 防御)。
//!
//! 见:`.planning/research/PITFALLS.md` Pitfall 1。

use crate::space_setup::IrohNodeConfig;

/// 把业务侧 `network.allow_relay_fallback` 翻译为 infra 侧 `IrohNodeConfig`。
///
/// 语义反转点(唯一):
/// - `allow_relay_fallback = true`  → `disable_relays = false`(允许 fallback)
/// - `allow_relay_fallback = false` → `disable_relays = true`(LAN-only)
///
/// `rendezvous_base_url`:`None` 走 `RENDEZVOUS_BASE_URL` 默认;production 调
/// 用方传 `None`;集成测试可覆盖。
pub(crate) fn relay_policy_to_iroh_config(
    allow_relay_fallback: bool,
    rendezvous_base_url: Option<String>,
) -> IrohNodeConfig {
    IrohNodeConfig {
        disable_relays: !allow_relay_fallback,
        rendezvous_base_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Truth-table 防御 Pitfall 1:任何方向写错都会被这两条断言抓住。
    #[test]
    fn allow_true_means_disable_false() {
        let cfg = relay_policy_to_iroh_config(true, None);
        assert_eq!(cfg.disable_relays, false);
        assert!(cfg.rendezvous_base_url.is_none());
    }

    #[test]
    fn allow_false_means_disable_true() {
        let cfg = relay_policy_to_iroh_config(false, None);
        assert_eq!(cfg.disable_relays, true);
    }

    #[test]
    fn rendezvous_override_passes_through() {
        let cfg = relay_policy_to_iroh_config(true, Some("http://test".into()));
        assert_eq!(cfg.rendezvous_base_url, Some("http://test".into()));
    }
}
```

**lib.rs 暴露模式 (基于 `lib.rs:8-17` 现有 `pub mod` 风格,`network_policy` 需为 `mod`,不 `pub mod`):**
```rust
// uc-bootstrap/src/lib.rs
pub mod assembly;
pub mod background_tasks;
pub mod builders;
// ...
mod network_policy;        // ← 新增,内部使用 pub(crate) 即可,不对外暴露
pub mod non_gui_runtime;
pub mod space_setup;
```

**改造动作:**
1. 新建 `uc-bootstrap/src/network_policy.rs`(40 行左右,含 truth-table 单测)
2. `lib.rs` 加 `mod network_policy;`(注意:不是 `pub mod`;`pub(crate)` 即可,§8/§9 调用方在同 crate 内)

**Pitfall 防御 (来自 PITFALLS.md Pitfall 1):**
- truth-table `(true→false, false→true)` 是 Pitfall 1 的核心防御;**两个测试都不能合并**(覆盖单一方向 + 反向独立断言)
- 函数命名 `relay_policy_to_iroh_config` 而非 `to_config`/`build_cfg`(语义自明,review 一眼能识别"这是翻译点")

---

### 11. **(NEW)** `src-tauri/crates/uc-infra/tests/lan_only_relay_mode.rs` (integration test)

**Analog:** `uc-infra/tests/iroh_presence_probe.rs` 全文(尤其 lines 17-42 的 bind 套路 + 直接 `Endpoint::builder` API);**不复用** `slice1_handshake_e2e.rs` 等大型 e2e 文件的 fixture(那些重在驱动 pairing 协议,本测试只验证 endpoint bind 时 `addrs` 内容)

**Loopback bind 模式 (`iroh_presence_probe.rs:17-29`):**
```rust
const PROBE_ALPN: &[u8] = b"uniclipboard/presence-probe/0";

/// Bind a single endpoint with relays disabled and `PROBE_ALPN` registered.
async fn bind_endpoint() -> Endpoint {
    Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![PROBE_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("bind endpoint")
}
```

**等候 addrs 发布模式 (`iroh_presence_probe.rs:34-42`):**
```rust
async fn wait_for_direct_addrs(endpoint: &Endpoint) {
    for _ in 0..100 {
        if !endpoint.addr().addrs.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("endpoint never published direct addresses");
}
```

**新增测试目标 (Tier B 自动化,验证 bind-time `RelayMode` 决策结果):**
```rust
//! LAN-only Mode bind-time 行为断言:验证 `IrohNodeConfig.disable_relays`
//! 通过 iroh `Endpoint::builder().relay_mode(...).bind()` 路径正确翻译为
//! `RelayMode::Disabled` / `RelayMode::Default`,效果体现在
//! `endpoint.addr().addrs` 是否含 `TransportAddr::Relay(_)` 项。
//!
//! 注意:loopback 测试无法验证"流量真没走 relay"(那是 Tier C 抓包验证);
//! 但能验证候选地址清单的方向正确,这是 Tier A/B 自动化覆盖的最深层。
//!
//! 见:`.planning/research/PITFALLS.md` Pitfall 8。

use std::time::Duration;
use iroh::{Endpoint, RelayMode, TransportAddr};

const TEST_ALPN: &[u8] = b"uniclipboard/lan-only-test/0";

async fn bind_with_relay_mode(mode: RelayMode) -> Endpoint {
    Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![TEST_ALPN.to_vec()])
        .relay_mode(mode)
        .bind()
        .await
        .expect("bind endpoint")
}

async fn wait_for_addrs(endpoint: &Endpoint) {
    for _ in 0..100 {
        if !endpoint.addr().addrs.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn relay_disabled_publishes_no_relay_addrs() {
    let endpoint = bind_with_relay_mode(RelayMode::Disabled).await;
    wait_for_addrs(&endpoint).await;
    let has_relay = endpoint
        .addr()
        .addrs
        .iter()
        .any(|a| matches!(a, TransportAddr::Relay(_)));
    assert!(
        !has_relay,
        "RelayMode::Disabled should not publish Relay addrs, got: {:?}",
        endpoint.addr().addrs
    );
    endpoint.close().await;
}

// 注意:RelayMode::Default 不一定立刻发布 Relay 地址(取决于 iroh 与
// 公网 relay mesh 的连通性,CI 环境可能没有公网),所以反向断言不写
// "must contain Relay";只断"Disabled 不应有 Relay"这一条强不等式。
// 这与 D-C1 的"两组用例"约定一致 —— 但反向用例可改写为:用
// `IrohNodeConfig { disable_relays: false, .. }` bind 后断言 *至少不抛
// 错* 即可,具体行为留给 Tier C 抓包验证。
```

**改造动作:**
1. 新建 `uc-infra/tests/lan_only_relay_mode.rs`(50 行左右)
2. 用 `RelayMode::Disabled` + `RelayMode::Default` 两组 bind
3. 断言 `endpoint.addr().addrs` 中是否含 `TransportAddr::Relay(_)` 项

**Pitfall 防御 (来自 PITFALLS.md Pitfall 8):**
- **不**复用 `IrohNodeConfig { disable_relays: true, .. }` 形式的 production-flavored config;直接用 iroh API,语义最直接
- 测试名 `relay_disabled_publishes_no_relay_addrs`(描述行为,非"测开关")— review 一眼能定位

**Cargo.toml 影响:** `uc-infra/Cargo.toml` 不需要改动 — `iroh` / `tokio test-util` 都已在 dev-dependencies(see `STACK.md §5.1`)

---

## 跨切面共享模式

### Pattern A: 反向命名只翻译一次 (Pitfall 1 防御)

**Source:** `uc-bootstrap/src/network_policy.rs::relay_policy_to_iroh_config()` (新增,§10)

**Apply to:** 全工程

**铁律:**
- 全工程只允许 **一处** 出现 `disable_relays = !allow_relay_fallback`(网络策略翻译点)
- DTO ↔ View ↔ core 三层只搬运 `allow_relay_fallback`(业务正向语义),不取反
- IPC wire 字段名 `allowRelayFallback`(camelCase),前端 store 不维护 `lanOnly` 镜像状态
- PR review 时跑 `rg 'disable_relays|allow_relay_fallback'` 守护这条不变量

**Excerpt:**
```rust
pub(crate) fn relay_policy_to_iroh_config(
    allow_relay_fallback: bool,
    rendezvous_base_url: Option<String>,
) -> IrohNodeConfig {
    IrohNodeConfig {
        disable_relays: !allow_relay_fallback,  // ← 全工程唯一一处
        rendezvous_base_url,
    }
}
```

---

### Pattern B: serde(default) + 手写 Default impl (Pitfall 2 防御)

**Source:** `uc-core/src/settings/model.rs:178-202` + `uc-core/src/settings/defaults.rs:227-262`

**Apply to:** `NetworkSettings` 定义 + `Settings.network` 挂载

**模式:**
1. `Settings.network` 字段标 `#[serde(default)]`(顶层挂载)
2. `NetworkSettings.allow_relay_fallback` 字段标 `#[serde(default = "default_allow_relay_fallback")]`(字段级)
3. `NetworkSettings` **禁止** `#[derive(Default)]`,必须在 `defaults.rs` 手写 `impl Default`
4. `default_allow_relay_fallback() -> bool { true }` 字面量上方加三行警示注释

**Excerpt (来自 `defaults.rs:215-225` `FileSyncSettings`,沿用同一手写模式):**
```rust
impl Default for FileSyncSettings {
    fn default() -> Self {
        Self {
            file_sync_enabled: true,
            // ...
        }
    }
}
```

**为什么(Pitfall 2):** Rust `#[derive(Default)]` 对 `bool` 默认 `false`;`NetworkSettings::default()` 若被自动 derive 会得到 `allow_relay_fallback: false` = LAN-only on = 老用户跨网段设备突然离线 = breaking change。手写 `true` + 注释警示是结构性防御。

---

### Pattern C: View / Patch 镜像 + apply_patch 末段追加 (Pitfall 历史欠账边界)

**Source:** `uc-application/src/facade/settings/models.rs:124-225, 446-566`

**Apply to:** `NetworkSettingsView` / `NetworkSettingsPatch` 定义 + `apply_settings_patch` 末段

**模式三步走:**
1. View(全字段必填,`Debug, Clone, PartialEq, Eq`)+ Patch(全字段 `Option<T>`,`Debug, Clone, Default`)
2. `From<core::Settings> for SettingsView` 末段映射
3. `apply_settings_patch` 末段 "only Some fields update" 分支

**Excerpt (来自 `models.rs:544-563` `file_sync` 段):**
```rust
if let Some(file_sync) = patch.file_sync {
    if let Some(v) = file_sync.file_sync_enabled {
        existing.file_sync.file_sync_enabled = v;
    }
    // ...
}
```

**为什么:** "only Some fields update" 是 NETSET-02 success criterion #2 的硬约束 — 旧客户端 PUT 不带 `network` 段时,行为应为 no-op,不抹掉已存在 `network` 字段。这是 patch 语义的核心。

---

### Pattern D: DTO `serde(rename_all = "camelCase")` (wire 契约)

**Source:** `uc-daemon-contract/src/api/dto/settings.rs:43-44, 71-73, 187-188` 等多处

**Apply to:** `NetworkSettingsDto` / `NetworkSettingsPatchDto` + `UpdateSettingsResponse.restart_required`

**模式:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettingsDto {
    pub allow_relay_fallback: bool,  // ← wire 字段 = "allowRelayFallback"
}
```

**为什么:** Rust 用 snake_case (`allow_relay_fallback`),JSON wire 用 camelCase (`allowRelayFallback`);项目 `SettingsDto` 整族都用 `#[serde(rename_all = "camelCase")]` 自动转换。前端 TypeScript 直接 import wire 类型,不用手写映射。

---

### Pattern E: facade `pub use` 白名单 (AGENTS.md §11.4 红线)

**Source:** `uc-application/src/facade/settings/mod.rs:5-12`

**Apply to:** `NetworkSettingsView, NetworkSettingsPatch` 暴露

**模式:**
```rust
pub use models::{
    // ... alphabetic 顺序 ...
    NetworkSettingsPatch, NetworkSettingsView,    // ← 插入到正确位置(在 GeneralSettings* 之后,Pairing* 之前)
    // ...
};
```

**为什么:** `uc-application/AGENTS.md` §11.4.7 明确:"新增类型必须显式 pub use,不允许下游直接从 `models.rs` import"。这是阻止 §11.4.7 历史欠账重发的唯一红线。

---

### Pattern F: tracing instrument + info! 启动日志 (D-B3)

**Source:** `uc-application/src/facade/settings/facade.rs:26, 35` (`#[instrument(skip_all)]`) + `uc-infra/src/network/iroh/node.rs:399-404` (`debug!(... disable_relays = config.disable_relays, ...)`)

**Apply to:** §8/§9 builders 改造点的日志输出

**模式 (info! 单行,字段固定):**
```rust
tracing::info!(
    target: "settings.network",   // 或不带 target,planner 决定(D-Discretion)
    allow_relay_fallback = settings.network.allow_relay_fallback,
    disable_relays = !settings.network.allow_relay_fallback,
    "applying network.allow_relay_fallback={value} → disable_relays={value}"
);
```

**字段名照抄 CONTEXT.md `<specifics>` 第三条:** `applying network.allow_relay_fallback={value} → disable_relays={value}`

**为什么:** 启动期必须有 audit log,方便 support 排障("我开了 LAN-only 重启了为什么还在走 relay?" → 查日志找 startup 这一行)。**不**在 OTLP 加 attribute(避免与 Pitfall 6 OTLP 不联动原则擦边)。

---

## 无 analog 的文件

无 — 所有 11 个文件都有可锚定的近邻模式(同文件 `FileSync*` 镜像 / 同 crate 装配点 / 同测试目录 loopback fixture),Phase 94 不需要从研究层 RESEARCH.md 抠模式。

---

## 元数据

**Analog 搜索范围:**
- `src-tauri/crates/uc-core/src/settings/`
- `src-tauri/crates/uc-application/src/facade/settings/`
- `src-tauri/crates/uc-daemon-contract/src/api/dto/`
- `src-tauri/crates/uc-webserver/src/api/`
- `src-tauri/crates/uc-bootstrap/src/`
- `src-tauri/crates/uc-bootstrap/tests/`
- `src-tauri/crates/uc-infra/src/network/iroh/`
- `src-tauri/crates/uc-infra/tests/`
- `src-tauri/crates/uc-infra/src/settings/`
- `src-tauri/crates/uc-core/src/ports/`

**已扫描文件数:** 19(全部为已 grep 验证的具体行号锚点;无开放性搜索)

**Pattern extraction date:** 2026-05-04

**关键依赖确认:**
- `iroh 0.98` API:`Endpoint::builder().relay_mode(...).bind()` + `endpoint.addr().addrs` + `TransportAddr::{Ip, Relay}` 全部已在项目代码使用(`node.rs:368-405`),版本锁在 `uc-infra/Cargo.toml:79`
- `serde_with` 派生模式:已在 `model.rs:1-5` 就位,无需新依赖
- `tokio test-util`:已在 `uc-infra/Cargo.toml:99`(`features = ["full", "test-util"]`),无需新依赖
- `SettingsMigrator::migrations` vec:`migration.rs:36-43` 当前为空(注释只写了示例 placeholder),Phase 94 **不**升 `CURRENT_SCHEMA_VERSION = 1`,vec 保持空

**关键不变量(PR review 必查):**
1. 全工程除 `uc-bootstrap/src/network_policy.rs` + `uc-infra/src/network/iroh/node.rs:153-162` + 测试文件外,无 `disable_relays = !` 类反向写法
2. `Settings.network.allow_relay_fallback` 默认 `true`(老 settings.json 反序列化必须 `== true`)
3. `apply_settings_patch` 处理 `network` 段时,patch.network = None 不抹掉已有字段值
4. `IrohNodeBuilder::bind` 一进程只能调一次(OnceCell 守护,Pitfall 3 结构性防御 — 实施在 §8 改造时可能不在 Phase 94 范围,planner 决策是否纳入)
5. `UpdateSettingsResponse.restart_required: bool` wire 契约就位,前端 Phase 95 同步消费

---

*Phase: 94-后端字段落地*
*Pattern map: 2026-05-04*
*Ready for planning: yes*
