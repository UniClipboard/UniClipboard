# Phase 95: 前端 NetworkSection + 重启 UX — Pattern Map

**Mapped:** 2026-05-04
**Files analyzed:** 12 个新建/修改文件
**Analogs found:** 12 / 12

---

## File Classification

| 新建/修改文件 | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `src/types/setting.ts` | type | transform | `src/types/setting.ts:110-133` (FileSyncSettings 块) | exact (扩展现有接口) |
| `src/api/daemon/settings.ts` | API client | request-response | `src/api/daemon/settings.ts:84-91,149-152,280-299` (FileSyncSettings 段) | exact |
| `src/contexts/SettingContext.tsx` | context provider | request-response | `src/contexts/SettingContext.tsx:123-143` (updateFileSyncSetting) | exact |
| `src/hooks/useDebounce.ts` | hook | transform | `src/hooks/useDebounce.ts:1-17` (现成，直接复用) | exact (已存在，无需新建) |
| `src/components/setting/NetworkSection.tsx` | component | request-response | `src/components/setting/SyncSection.tsx:16-70` (Switch + useSetting 模式) | exact |
| `src/components/setting/RestartBanner.tsx` | component | event-driven | `src/components/setting/SyncSection.tsx:204-209` (error 渲染) + `src/components/ui/button.tsx` | role-match |
| `src/components/setting/LanOnlyDisclosure.tsx` | component | request-response | `src/components/ui/popover.tsx:1-31` (Popover primitive) | role-match |
| `src-tauri/crates/uc-tauri/src/commands/restart.rs` | Tauri command | request-response | `src-tauri/crates/uc-tauri/src/commands/storage.rs:1-43` | exact |
| `src-tauri/crates/uc-tauri/src/commands/mod.rs` | config | — | `src-tauri/crates/uc-tauri/src/commands/mod.rs:1-44` (pub use + generate_handler) | exact |
| `src-tauri/crates/uc-tauri/src/run.rs` (修改) | config | — | `src-tauri/crates/uc-tauri/src/run.rs:416-444` (generate_handler 列表) | exact |
| `src/i18n/locales/zh-CN.json` (修改) | i18n bundle | — | `src/i18n/locales/zh-CN.json:192-237` (旧 network 块 → 替换) | exact |
| `src/i18n/locales/en-US.json` (修改) | i18n bundle | — | `src/i18n/locales/en-US.json:201-247` (对等旧 network 块 → 替换) | exact |

---

## Pattern Assignments

### `src/types/setting.ts` (type, transform)

**Analog:** `src/types/setting.ts:110-133` (FileSyncSettings + Settings interface)

**需要做的两处修改：**

1. 在 `FileSyncSettings` 接口之后新增 `NetworkSettings` 接口
2. 在 `Settings` interface 里加 `network` 必填字段
3. 在 `SettingContextType` 里加 `updateNetworkSetting`

**接口扩展模式** (`src/types/setting.ts:110-133`):
```
export interface FileSyncSettings {
  fileSyncEnabled: boolean
  smallFileThreshold: number
  maxFileSize: number
  fileCacheQuotaPerDevice: number
  fileRetentionHours: number
  fileAutoCleanup: boolean
}

export interface Settings {
  schemaVersion: number
  general: GeneralSettings
  sync: SyncSettings
  ...
  fileSync?: FileSyncSettings   // ← Phase 95: network: NetworkSettings 仿此形态，但必填（非 optional）
}
```

**SettingContextType 扩展模式** (`src/types/setting.ts:181-192`):
```
export interface SettingContextType {
  setting: Settings | null
  loading: boolean
  error: string | null
  updateSetting: (newSetting: Settings) => Promise<void>
  updateGeneralSetting: (newGeneralSetting: Partial<GeneralSettings>) => Promise<void>
  updateSyncSetting: (newSyncSetting: Partial<SyncSettings>) => Promise<void>
  ...
  updateFileSyncSetting: (newFileSyncSetting: Partial<FileSyncSettings>) => Promise<void>
  // ↑ Phase 95 新增: updateNetworkSetting: (newNetworkSetting: Partial<NetworkSettings>) => Promise<{ restartRequired: boolean }>
}
```

**新增内容（紧贴 FileSyncSettings 之后）：**
```typescript
/** Network settings — corresponds to Rust NetworkSettings / uc-daemon-contract NetworkSettingsDto */
export interface NetworkSettings {
  allowRelayFallback: boolean   // camelCase：与 daemon serde(rename_all="camelCase") 对齐
}
```

---

### `src/api/daemon/settings.ts` (API client, request-response)

**Analog:** `src/api/daemon/settings.ts` 的 FileSyncSettings 段

**4 处修改：**

**1. 新增 NetworkSettings 接口** (仿 `src/api/daemon/settings.ts:84-91`):
```
export interface FileSyncSettings {
  fileSyncEnabled: boolean
  ...
}
// ↓ 在此位置新增：
export interface NetworkSettings {
  allowRelayFallback: boolean
}
```

**2. Settings interface 加 network 字段** (仿 `src/api/daemon/settings.ts:129-138`):
```
export interface Settings {
  ...
  fileSync: FileSyncSettings
  network: NetworkSettings   // ← 新增，必填
}
```

**3. SettingsUpdateResponse 加 restartRequired** (当前 `src/api/daemon/settings.ts:149-152`):
```
// 当前：
interface SettingsUpdateResponse {
  data: { success: boolean }
  ts: number
}
// Phase 95 改为：
interface SettingsUpdateResponse {
  data: { success: boolean; restartRequired: boolean }
  ts: number
}
```

**4. updateSettings 返回值 + toSettingsPatchRequest 扩展** (仿 `src/api/daemon/settings.ts:201-207, 280-299`):

```
// 当前签名（src/api/daemon/settings.ts:201-207）：
export async function updateSettings(settings: Partial<Settings>): Promise<void> {
  const patch = toSettingsPatchRequest(settings)
  await daemonClient.request<SettingsUpdateResponse>('/settings', {
    method: 'PUT',
    body: patch,
  })
}

// Phase 95 改为：
export async function updateSettings(
  settings: Partial<Settings>
): Promise<{ success: boolean; restartRequired: boolean }> {
  const patch = toSettingsPatchRequest(settings)
  const res = await daemonClient.request<SettingsUpdateResponse>('/settings', {
    method: 'PUT',
    body: patch,
  })
  return { success: res.data.success, restartRequired: res.data.restartRequired }
}
```

**toSettingsPatchRequest 新增 network 段**（仿 `src/api/daemon/settings.ts:280-299` fileSync 段）:
```
// src/api/daemon/settings.ts:280-299（fileSync 段，Phase 95 仿此添加 network 段）：
if (settings.fileSync) {
  const { fileSyncEnabled, smallFileThreshold, ... } = settings.fileSync
  patch.fileSync = { fileSyncEnabled, smallFileThreshold, ... }
}
// ↓ Phase 95 在此之后新增：
if (settings.network) {
  patch.network = { allowRelayFallback: settings.network.allowRelayFallback }
}
```

---

### `src/contexts/SettingContext.tsx` (context provider, request-response)

**Analog:** `src/contexts/SettingContext.tsx:123-143` (updateFileSyncSetting 模式)

**3 处修改：**

**1. saveSetting 改为返回 restartRequired** (当前 `src/contexts/SettingContext.tsx:46-64`):
```
// 当前：
const saveSetting = async (newSetting: Settings) => {
  try {
    setLoading(true)
    await updateSettings(newSetting)          // 丢弃返回值
    setSetting(newSetting)
    ...
  } catch (err) {
    log.error({ err }, '保存设置失败')
    setError(`保存设置失败: ${err}`)
    throw err
  } finally {
    setLoading(false)
  }
}

// Phase 95 改为（返回 { restartRequired }）：
const saveSetting = async (newSetting: Settings): Promise<{ restartRequired: boolean }> => {
  try {
    setLoading(true)
    const result = await updateSettings(newSetting)   // 读取返回值
    setSetting(newSetting)
    setError(null)
    try { await emitSettingsChanged(newSetting) } catch (err) { ... }
    return { restartRequired: result.restartRequired }
  } catch (err) {
    log.error({ err }, '保存设置失败')
    setError(`保存设置失败: ${err}`)
    throw err
  } finally {
    setLoading(false)
  }
}
```

**2. 新增 updateNetworkSetting helper** (完整镜像 `src/contexts/SettingContext.tsx:123-143`):
```
// src/contexts/SettingContext.tsx:123-143 (updateFileSyncSetting — Phase 95 镜像模式)：
const updateFileSyncSetting = async (
  newFileSyncSetting: Partial<Settings['fileSync'] & object>
) => {
  if (!setting) return
  const updatedSetting: Settings = {
    ...setting,
    fileSync: {
      ...(setting.fileSync ?? { /* defaults */ }),
      ...newFileSyncSetting,
    },
  }
  await saveSetting(updatedSetting)
}

// Phase 95 对应新增（updateNetworkSetting，返回 restartRequired）：
const updateNetworkSetting = async (
  newNetworkSetting: Partial<NetworkSettings>
): Promise<{ restartRequired: boolean }> => {
  if (!setting) return { restartRequired: false }
  const updatedSetting: Settings = {
    ...setting,
    network: {
      ...setting.network,
      ...newNetworkSetting,
    },
  }
  return await saveSetting(updatedSetting)
}
```

**3. value 对象中注册** (仿 `src/contexts/SettingContext.tsx:242-253`):
```
const value: SettingContextType = {
  setting, loading, error,
  updateSetting, updateGeneralSetting, updateSyncSetting,
  updateSecuritySetting, updateRetentionPolicy, updateKeyboardShortcuts,
  updateFileSyncSetting,
  updateNetworkSetting,   // ← 新增
}
```

---

### `src/components/setting/NetworkSection.tsx` (component, request-response)

**Analog:** `src/components/setting/SyncSection.tsx:16-70` (Switch + useSetting + debounce 模式)

**完全重写。** 当前占位 (`src/components/setting/NetworkSection.tsx:1-25`) 全删。

**组件签名模式** (仿 `src/components/setting/SyncSection.tsx:16-19`):
```
// SyncSection.tsx:16-19 — 签名模式
const SyncSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateSyncSetting, updateFileSyncSetting } = useSetting()
```

**useDebounce 集成模式** (不同于 SyncSection，Phase 95 显式偏离 — 见 CONTEXT.md D-D3):
```
// NetworkSection 特有：Switch 状态与 debounce 写盘解耦
const [allowRelayFallback, setAllowRelayFallback] = useState(
  setting?.network?.allowRelayFallback ?? true
)
const [pending, setPending] = useState(false)
const debouncedAllowRelay = useDebounce(allowRelayFallback, 500)

// useEffect 监听 debouncedAllowRelay 触发 PUT（与 SyncSection 直接 call 不同）
useEffect(() => {
  if (!setting) return
  void updateNetworkSetting({ allowRelayFallback: debouncedAllowRelay })
}, [debouncedAllowRelay])
```

**Switch + SettingRow 模式** (仿 `src/components/setting/SyncSection.tsx:215-220`):
```
// SyncSection.tsx:215-220
<SettingRow
  label={t('settings.sections.sync.autoSync.label')}
  description={t('settings.sections.sync.autoSync.description')}
>
  <Switch id="auto-sync" checked={autoSync} onCheckedChange={handleAutoSyncChange} />
</SettingRow>

// NetworkSection 对应（含 labelExtra slot 装 info-icon）：
<SettingRow
  label={t('settings.sections.network.lanOnly.label')}
  labelExtra={<LanOnlyDisclosure />}
  description={t('settings.sections.network.lanOnly.description')}
>
  <Switch
    id="lan-only-switch"
    checked={!allowRelayFallback}              // 反向命名：checked=ON = allowRelay=false
    onCheckedChange={(checked) => {
      setAllowRelayFallback(!checked)          // 仅一处取反 — ROADMAP 防御
      setPending(true)
    }}
  />
</SettingRow>
```

**error state 渲染模式** (仿 `src/components/setting/SyncSection.tsx:204-209`):
```
// SyncSection.tsx:204-209
if (error) {
  return (
    <div className="text-destructive py-4">
      {t('settings.sections.sync.loadError')} {error}
    </div>
  )
}
```

**SettingGroup 容器模式** (仿 `src/components/setting/SyncSection.tsx:213-235`):
```
// SyncSection.tsx:213-235
return (
  <>
    <SettingGroup title={t('settings.categories.sync')}>
      <SettingRow ...>...</SettingRow>
    </SettingGroup>
  </>
)

// NetworkSection：SettingGroup 内 RestartBanner 为第一子节点
return (
  <SettingGroup title={t('settings.categories.network')}>
    <RestartBanner
      visible={pending}
      onRestart={handleRestart}
      loading={restartLoading}
      error={restartError}
      onDismissError={() => setRestartError(null)}
    />
    <SettingRow ...>
      <Switch ... />
    </SettingRow>
  </SettingGroup>
)
```

---

### `src/components/setting/RestartBanner.tsx` (component, event-driven)

**Analog:** 无完全匹配的 inline banner 组件。最近参考：`src/components/setting/SyncSection.tsx:204-209` (error div 模式) + shadcn `Button` + lucide-react `RefreshCw`。

**UI-SPEC Layout Contract 给出的精确 DOM 结构：**

```
// RestartBanner.tsx — 从 UI-SPEC.md Layout Contract 直接导出
<div
  role="status"
  aria-live="polite"
  className="flex items-start gap-2 px-4 py-3 bg-accent/40 border-b border-border/40 rounded-none"
>
  <RefreshCw className="size-4 text-foreground mt-0.5 shrink-0" />
  <div className="flex-1 space-y-1">
    <p className="text-sm text-foreground">{message}</p>
    {error && (
      <p role="alert" className="text-xs text-destructive">{error}</p>
    )}
  </div>
  <div className="ml-auto flex items-center gap-2">
    {!error ? (
      <Button variant="default" size="sm" onClick={onRestart} disabled={loading}>
        {loading ? t('...restartingButton') : t('...restartButton')}
      </Button>
    ) : (
      <>
        <Button variant="outline" size="sm" onClick={onRestart} disabled={loading}>
          {t('...retryButton')}
        </Button>
        <Button variant="ghost" size="icon" aria-label={t('...dismissAriaLabel')}
          onClick={onDismissError}>
          <X className="size-3.5" />
        </Button>
      </>
    )}
  </div>
</div>
```

**Props 接口：**
```typescript
interface RestartBannerProps {
  visible: boolean
  onRestart: () => Promise<void>
  loading?: boolean
  error?: string | null
  onDismissError?: () => void
}
```

**Button primitive** (`src/components/ui/button.tsx`): `variant="default"` (primary fill for CTA) + `variant="outline"` (retry) + `variant="ghost" size="icon"` (dismiss X)。

**可见性控制：** 父组件用 `{visible && <RestartBanner ... />}` 控制挂载/卸载，无 CSS opacity 过渡（pending 状态不可 dismiss）。

---

### `src/components/setting/LanOnlyDisclosure.tsx` (component, request-response)

**Analog:** `src/components/ui/popover.tsx:1-31` (Popover primitive 完整导出)

**Popover 使用模式** (analog: `src/components/ui/popover.tsx:5-31`):
```
// src/components/ui/popover.tsx:5-7 — 三个导出名称
const Popover = PopoverPrimitive.Root
const PopoverTrigger = PopoverPrimitive.Trigger
// PopoverContent 默认 align="center" sideOffset={8} w-72 rounded-xl border bg-popover p-3

// LanOnlyDisclosure 使用模式（UI-SPEC D-C1）：
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover'
import { Info } from 'lucide-react'

export function LanOnlyDisclosure() {
  const { t } = useTranslation()
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={t('settings.sections.network.lanOnly.infoIconAriaLabel')}
          aria-haspopup="dialog"
          className="inline-flex items-center justify-center rounded-md p-1
                     text-muted-foreground hover:text-foreground hover:bg-accent"
        >
          <Info className="size-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" sideOffset={8}>
        {/* 标题 + intro + 4 items */}
      </PopoverContent>
    </Popover>
  )
}
```

**Popover 内容结构** (4 条清单，每条 title + description):
```
<div className="space-y-3">
  <div>
    <p className="text-sm font-medium">{t('...disclosure.title')}</p>
    <p className="text-xs text-muted-foreground mt-1">{t('...disclosure.intro')}</p>
  </div>
  <div className="space-y-2">
    {['rendezvous','otlp','pkarr','autoUpdate'].map(key => (
      <div key={key} className="space-y-1">
        <p className="text-sm font-medium">{t(`...disclosure.${key}.title`)}</p>
        <p className="text-xs text-muted-foreground leading-snug">
          {t(`...disclosure.${key}.description`)}
        </p>
      </div>
    ))}
  </div>
</div>
```

---

### `src-tauri/crates/uc-tauri/src/commands/restart.rs` (Tauri command, request-response)

**Analog:** `src-tauri/crates/uc-tauri/src/commands/storage.rs:1-43` (完整 Tauri command 文件模式)

**文件头 + use 声明** (仿 `src-tauri/crates/uc-tauri/src/commands/storage.rs:1-9`):
```rust
//! Restart-related Tauri commands
//! 重启相关的 Tauri 命令

use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use std::time::SystemTime;
use tracing::{info, info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;
```

**`restart_app` command** (仿 storage.rs + updater.rs:300-301 的 app.restart() 调用):
```rust
// updater.rs:300-301 — 现有 app.restart() 调用模式（Phase 95 复用）：
info!("update installed, restarting app");
app.restart();

// Phase 95 新 command（仿 storage.rs 结构）：
#[tauri::command]
pub async fn restart_app(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.restart.restart_app",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        info!("restarting app for settings change");
        app.restart();
        // app.restart() 不返回（进程退出），以下代码不可达
        Ok(())
    }
    .instrument(span)
    .await
}
```

**`get_restart_state` command** (新 Serde 返回类型 + OnceCell 读取):
```rust
// get_daemon_connection_info 返回 Serde 结构体模式（startup.rs:16-22）：
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonConnectionPayload { ... }

// Phase 95 对应（RestartState 返回 millis）：
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartState {
    process_started_at: i64,   // millis since epoch — PROCESS_STARTED_AT OnceCell
    settings_mtime: i64,       // std::fs::metadata(settings_path).modified() millis
}

#[tauri::command]
pub async fn get_restart_state(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<RestartState, CommandError> {
    let span = info_span!(
        "command.restart.get_restart_state",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let settings_path = runtime.storage_paths().settings_path.clone();
        let settings_mtime = std::fs::metadata(&settings_path)
            .and_then(|m| m.modified())
            .map(|t| t.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default().as_millis() as i64)
            .map_err(|e| CommandError::internal(e))?;
        let process_started_at = PROCESS_STARTED_AT.get()
            .map(|t| t.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default().as_millis() as i64)
            .unwrap_or(0);
        Ok(RestartState { process_started_at, settings_mtime })
    }
    .instrument(span)
    .await
}
```

**`PROCESS_STARTED_AT` OnceCell** (在 restart.rs 顶层声明，`uc_tauri::run` 启动早期 set):
```rust
use std::sync::OnceLock;
pub static PROCESS_STARTED_AT: OnceLock<SystemTime> = OnceLock::new();
```

**`runtime.storage_paths().settings_path`** 路径来源: `src-tauri/crates/uc-application/src/facade/app_paths.rs:50` 确认为 `app_data_root.join("settings.json")`，通过 `runtime.storage_paths()` 暴露 (仿 `src-tauri/crates/uc-tauri/src/commands/storage.rs:26`)。

---

### `src-tauri/crates/uc-tauri/src/commands/mod.rs` (config, —)

**Analog:** `src-tauri/crates/uc-tauri/src/commands/mod.rs:1-44` (现有文件，新增 restart 模块)

**当前文件结构** (`src-tauri/crates/uc-tauri/src/commands/mod.rs:1-7`):
```rust
pub mod autostart;
pub mod error;
pub mod quick_panel;
pub mod startup;
pub mod storage;
pub mod tray;
pub mod updater;
```

**Phase 95 新增两行：**
```rust
pub mod restart;    // ← 新增
// ... 现有其余 pub mod ...

pub use restart::*;  // ← 新增（与 pub use storage::*; 同行距）
```

---

### `src-tauri/crates/uc-tauri/src/run.rs` (config, —)

**Analog:** `src-tauri/crates/uc-tauri/src/run.rs:416-444` (generate_handler 列表)

**当前注册列表片段** (`src-tauri/crates/uc-tauri/src/run.rs:416-444`):
```rust
.invoke_handler(tauri::generate_handler![
    // Lifecycle commands
    crate::commands::get_tauri_pid,
    crate::commands::get_device_id,
    crate::commands::get_daemon_connection_info,
    // Autostart commands
    crate::commands::autostart::enable_autostart,
    ...
    // Updater commands
    crate::commands::updater::check_for_update,
    crate::commands::updater::install_update,
    // Storage commands
    crate::commands::storage::open_data_directory,
    ...
])
```

**Phase 95 在 Lifecycle commands 块新增两行：**
```rust
// Restart commands
crate::commands::restart::restart_app,
crate::commands::restart::get_restart_state,
```

**以及在 `uc_tauri::run` 函数早期（setup 阶段前）初始化 OnceCell：**
```rust
// 在 builder.setup(...) 内部、manage() 调用之前：
let _ = crate::commands::restart::PROCESS_STARTED_AT.set(SystemTime::now());
```

---

### `src/i18n/locales/zh-CN.json` + `en-US.json` (i18n bundle, —)

**Analog:** 整个 `settings.sections.network` 块（`zh-CN.json:192-237`）— **删除旧块，写入新块**。

**删除目标** (旧残留键，`zh-CN.json:192-237`):
```
"network": {
  "title": ..., "syncMethod": {...}, "webserverPort": {...},
  "customPeerDevice": {...}, "cloudServer": {...}, "loadError": ...
}
```

**替换写入的新结构**（完整 i18n key 树，来源 UI-SPEC.md §Copywriting Contract）:
```json
"network": {
  "lanOnly": {
    "label": "LAN-only 模式",
    "description": "关闭公网中继回落，仅通过同局域网完成设备同步。重启后生效。",
    "infoIconAriaLabel": "查看 LAN-only 模式开启后仍走外网的请求清单",
    "saveError": "保存失败：{{message}}。已恢复到上一次的设置。",
    "disclosure": {
      "title": "LAN-only 开启后仍会走外网的请求",
      "intro": "以下 4 类请求由独立模块控制，不受 LAN-only 影响：",
      "rendezvous": {
        "title": "首次配对 rendezvous",
        "description": "配对新设备时仍需联网经 `rendezvous.uniclipboard.app` 完成 NodeId 交换；已配对设备的日常同步不再使用 rendezvous。"
      },
      "otlp": {
        "title": "OTLP 遥测",
        "description": "遥测由「通用 → 遥测」开关独立控制，与 LAN-only 无关；如需关闭，请到「通用」分类。"
      },
      "pkarr": {
        "title": "pkarr DHT NodeId 解析",
        "description": "跨网段连接通过 pkarr 公网 DHT 解析对端 NodeId，性质类似 DNS。关闭会导致跨网段连接率从约 90% 跌至接近 0。"
      },
      "autoUpdate": {
        "title": "自动更新 GitHub 检查",
        "description": "由「通用 → 自动更新」开关独立控制，访问 GitHub Release API 检查新版本。"
      }
    }
  },
  "restartBanner": {
    "message": "需要重启应用以使 LAN-only 模式更改生效。",
    "restartButton": "立即重启",
    "restartingButton": "正在重启…",
    "dismissAriaLabel": "收起重启提示",
    "errorMessage": "自动重启失败，请手动退出并重新打开应用以使更改生效。",
    "retryButton": "重试"
  }
}
```

**en-US.json 对等结构**（键名相同，值为英文，来源 UI-SPEC.md）:
- `"label": "LAN-only Mode"`
- `"description": "Disable public-relay fallback. Devices sync only over the same local network. Takes effect after restart."`
- `"message": "Restart the app to apply the LAN-only Mode change."`
- `"restartButton": "Restart now"` / `"restartingButton": "Restarting…"`
- `"retryButton": "Retry"` / `"dismissAriaLabel": "Dismiss restart notice"`
- `"errorMessage": "Automatic restart failed. Please quit the app manually and reopen it to apply the change."`
- 4 条 disclosure 项：见 UI-SPEC.md §Info-icon Popover 表格（en-US 列）

---

## Shared Patterns

### invokeWithTrace — Tauri command 调用
**Source:** `src/lib/tauri-command.ts:19-50`
**Apply to:** NetworkSection（`get_restart_state` + `restart_app` 调用）

```typescript
// src/lib/tauri-command.ts:19-50 — 统一 Tauri 调用入口
export async function invokeWithTrace<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T>

// NetworkSection 使用示例：
const state = await invokeWithTrace<{ processStartedAt: number; settingsMtime: number }>(
  'get_restart_state'
)
await invokeWithTrace<void>('restart_app')
```

### record_trace_fields — Rust 命令 tracing 注入
**Source:** `src-tauri/crates/uc-tauri/src/commands/mod.rs:38-43`
**Apply to:** `restart.rs` 内所有 `#[tauri::command]` 函数

```rust
// src-tauri/crates/uc-tauri/src/commands/mod.rs:38-43
pub(crate) fn record_trace_fields(span: &Span, trace: &Option<TraceMetadata>) {
    if let Some(metadata) = trace.as_ref() {
        span.record("trace_id", tracing::field::display(&metadata.trace_id));
        span.record("trace_ts", metadata.timestamp);
    }
}
```

### info_span! + Instrument — Rust async command 追踪
**Source:** `src-tauri/crates/uc-tauri/src/commands/storage.rs:18-22` + `startup.rs:48-57`

```rust
let span = info_span!(
    "command.restart.restart_app",
    trace_id = tracing::field::Empty,
    trace_ts = tracing::field::Empty,
);
record_trace_fields(&span, &_trace);
async move { ... }.instrument(span).await
```

### CommandError — Rust 命令错误类型
**Source:** `src-tauri/crates/uc-tauri/src/commands/error.rs:1-33`
**Apply to:** `restart.rs`（`get_restart_state` 读取 metadata 失败时返回 `CommandError::internal(e)`）

```rust
// error.rs:8-26
pub enum CommandError {
    #[error("not found: {0}")]       NotFound(String),
    #[error("internal error: {0}")] InternalError(String),
    ...
}
impl CommandError {
    pub fn internal(err: impl std::fmt::Display) -> Self {
        CommandError::InternalError(err.to_string())
    }
}
```

### storage_paths().settings_path — 路径 helper
**Source:** `src-tauri/crates/uc-tauri/src/commands/storage.rs:26` + `src-tauri/crates/uc-application/src/facade/app_paths.rs:9,50`
**Apply to:** `restart.rs::get_restart_state`

```rust
// storage.rs:26 — 路径访问模式
let dir = runtime.storage_paths().app_data_root_dir.clone();
// restart.rs 对应：
let settings_path = runtime.storage_paths().settings_path.clone();
// app_paths.rs:50 确认 settings_path = app_data_root.join("settings.json")
```

### useTranslation + t() — 前端 i18n
**Source:** `src/components/setting/SyncSection.tsx:17` + `src/components/setting/NetworkSection.tsx:2`
**Apply to:** RestartBanner、LanOnlyDisclosure、NetworkSection

```typescript
// 所有 Section 组件顶部固定模式：
const { t } = useTranslation()
// 使用：t('settings.sections.network.lanOnly.label')
```

### useSetting() — context 接入
**Source:** `src/hooks/useSetting.ts:10-16`
**Apply to:** NetworkSection（解构 `setting`, `error`, `updateNetworkSetting`）

```typescript
const { setting, error, updateNetworkSetting } = useSetting()
```

### useEffect + setting 同步 — 本地 state 随 context 更新
**Source:** `src/components/setting/SyncSection.tsx:52-64`
**Apply to:** NetworkSection

```typescript
// SyncSection.tsx:52-64
useEffect(() => {
  if (setting) {
    setAutoSync(setting.sync.autoSync)
    ...
  }
}, [setting])
// NetworkSection 对应：
useEffect(() => {
  if (setting?.network) {
    setAllowRelayFallback(setting.network.allowRelayFallback)
  }
}, [setting])
```

---

## No Analog Found

| 文件/模式 | Role | Data Flow | 原因 |
|---|---|---|---|
| `PROCESS_STARTED_AT: OnceLock<SystemTime>` | global state | — | 项目中无现有 process-lifetime OnceCell/OnceLock；最近参考是 Phase 94 plan 06 的 `IrohNodeBuilder` OnceCell（不在前端路径） |
| Component mount 调 `get_restart_state` + 推导 pending 逻辑 | hook/component | event-driven | 无已有组件在 mount 时查 Tauri state 推断 pending；planner 参考 CONTEXT.md D-D1 定义的推导规则 (`settings_mtime > process_started_at` ⇒ pending) |

---

## Metadata

**Analog search scope:** `src/` (components/setting, contexts, hooks, api/daemon, lib, i18n), `src-tauri/crates/uc-tauri/src/commands/`
**Files scanned:** 14 个源文件完整读取
**Pattern extraction date:** 2026-05-04
