// ============================================================================
// 新架构类型定义 - 与 Rust 后端 uc-core/src/settings/model.rs 完全匹配
// ============================================================================

/**
 * 主题模式 - 对应 Rust Theme enum
 */
export type Theme = 'light' | 'dark' | 'system'

/**
 * 更新频道 - 对应 Rust UpdateChannel enum
 */
export type UpdateChannel = 'stable' | 'alpha' | 'beta' | 'rc'

/**
 * 通用设置 - 对应 Rust GeneralSettings
 *
 * # themeColor 字段拆分（v0.7+）
 * `themeColor` 为旧版"统一主题预设"字段,新前端不再写入,但读取时仍作为
 * `themeColorLight` / `themeColorDark` 都为 null 时的回退,以保留 v0.7
 * 之前持久化的偏好。
 */
export interface GeneralSettings {
  autoStart: boolean
  silentStart: boolean
  autoCheckUpdate: boolean
  /**
   * Whether to download the next available update in the background.
   * Pre-fetching the installer bytes lets the click-to-install flow skip
   * the download step entirely. Opt-in: defaults to `false`. UI gates
   * this toggle on `autoCheckUpdate` so users can't get into
   * "download but never check" combinations.
   */
  autoDownloadUpdate: boolean
  theme: Theme
  /** 旧版统一主题预设字段(读取时作为回退,新代码不写入)。 */
  themeColor: string | null
  /** Light 模式下的主题预设名,如 `"zinc"`、`"catppuccin"`。 */
  themeColorLight: string | null
  /** Dark 模式下的主题预设名,如 `"zinc"`、`"catppuccin"`。 */
  themeColorDark: string | null
  /** Light 模式下用户对预设 token 的自定义覆盖（key = token 名, value = oklch 字符串）。
   *  允许的 key:`primary` | `background` | `foreground` | `border`。 */
  themeOverridesLight: Record<string, string>
  /** Dark 模式下用户对预设 token 的自定义覆盖（语义同 light）。 */
  themeOverridesDark: Record<string, string>
  language: string | null
  deviceName: string | null
  updateChannel?: UpdateChannel | null
  telemetryEnabled: boolean
  usageAnalyticsEnabled: boolean
}

/**
 * 内容类型 - 对应 Rust ContentTypes
 */
export interface ContentTypes {
  text: boolean
  image: boolean
  link: boolean
  file: boolean
  codeSnippet: boolean
  richText: boolean
}

/**
 * 同步频率 - 对应 Rust SyncFrequency enum
 */
export type SyncFrequency = 'realtime' | 'interval'

/**
 * 同步设置 - 对应 Rust SyncSettings
 */
export interface SyncSettings {
  autoSync: boolean
  syncFrequency: SyncFrequency
  contentTypes: ContentTypes
}

/**
 * 持续时间表示 - 对应 Rust Duration
 * Rust serde_with::DurationSeconds<u64> 将 Duration 序列化为秒数
 */
export type DurationSeconds = number

/**
 * 保留规则 - 对应 Rust RetentionRule enum
 * Rust 使用 serde externally-tagged + camelCase
 * 序列化为 { "byAge": { "maxAge": 2592000 } } 格式
 */
export type RetentionRule =
  | { byAge: { maxAge: DurationSeconds } }
  | { byCount: { maxItems: number } }
  | { byContentType: { contentType: ContentTypes; maxAge: DurationSeconds } }
  | { byTotalSize: { maxBytes: number } }
  | { sensitive: { maxAge: DurationSeconds } }

/**
 * 规则评估方式 - 对应 Rust RuleEvaluation enum
 */
export type RuleEvaluation = 'anyMatch' | 'allMatch'

/**
 * 保留策略 - 对应 Rust RetentionPolicy
 */
export interface RetentionPolicy {
  enabled: boolean
  rules: RetentionRule[]
  skipPinned: boolean
  evaluation: RuleEvaluation
}

/**
 * 安全设置 - 对应 Rust SecuritySettings
 */
export interface SecuritySettings {
  encryptionEnabled: boolean
  passphraseConfigured: boolean
  autoUnlockEnabled: boolean
}

/**
 * 配对设置 - 对应 Rust PairingSettings
 */
export interface PairingSettings {
  stepTimeout: DurationSeconds
  userVerificationTimeout: DurationSeconds
  sessionTimeout: DurationSeconds
  maxRetries: number
  protocolVersion: string
}

/**
 * File sync settings - corresponds to Rust FileSyncSettings
 */
export interface FileSyncSettings {
  fileSyncEnabled: boolean
  smallFileThreshold: number // bytes, default 10MB
  maxFileSize: number // bytes, default 5GB
  fileCacheQuotaPerDevice: number // bytes, default 500MB
  fileRetentionHours: number // default 24
  fileAutoCleanup: boolean // default true
}

/**
 * 网络设置 — 对应 Rust NetworkSettings / uc-daemon-contract NetworkSettingsDto。
 *
 * # 反向命名规则（Pitfall 1 防御）
 * UI checked = "LAN-only Mode = ON" 等价于 allowRelayFallback 取反值。
 * 前端只允许在 NetworkSection.tsx 一处用取反表达式；
 * 永远不要在前端 store 维护反向布尔镜像字段。
 *
 * `allowOverlayNetworkAddrs` 为正向同名字段（UI checked === 字段值），
 * 控制是否把 VPN/overlay 类虚拟网卡 IP 作为 iroh 直连候选。
 *
 * `customRelayUrls` 为空时使用 iroh 默认中继；非空时只使用这些自定义
 * relay URL。LAN-only 模式开启时列表保留但不生效。
 */
export interface NetworkSettings {
  allowRelayFallback: boolean
  allowOverlayNetworkAddrs: boolean
  customRelayUrls: string[]
}

/**
 * 快捷面板（Spotlight 风格）功能开关 - 对应 Rust QuickPanelSettings
 *
 * 默认 `enabled = true`：快捷面板是产品核心交互入口，新装即应可用。
 * 变更走 `set_quick_panel_enabled` Tauri command：开启即时注册全局快捷键
 * 并预创建隐藏窗口；关闭即时反注册快捷键，但窗口与底层 WebContent 进程
 * 留到 GUI 重启后才彻底释放（macOS 上销毁路径会崩溃）。
 */
export interface QuickPanelSettings {
  enabled: boolean
}

/**
 * 应用设置 - 对应 Rust Settings
 */
export interface Settings {
  schemaVersion: number
  general: GeneralSettings
  sync: SyncSettings
  retentionPolicy: RetentionPolicy
  security: SecuritySettings
  pairing: PairingSettings
  keyboardShortcuts?: Record<string, string | string[]>
  fileSync?: FileSyncSettings
  network: NetworkSettings
  quickPanel: QuickPanelSettings
}

// ============================================================================
// 向后兼容的类型别名 (用于旧代码)
// ============================================================================

/** @deprecated 使用 GeneralSettings 替代 */
export type GeneralSetting = GeneralSettings

/** @deprecated 使用 SyncSettings 替代 */
export type SyncSetting = SyncSettings

/** @deprecated 使用 SecuritySettings 替代，注意字段名不同 */
export interface SecuritySetting {
  end_to_end_encryption: boolean
  password: string
}

// ============================================================================
// 旧架构的类型 (保留用于向后兼容，但后端已不再返回这些字段)
// ============================================================================

/** @deprecated 后端新架构中不存在此设置 */
export interface NetworkSetting {
  sync_method: string
  cloud_server: string
  webserver_port: number
  custom_peer_device: boolean
  peer_device_addr: string | null
  peer_device_port: number | null
}

/** @deprecated 后端新架构中不存在此设置 */
export interface StorageSetting {
  auto_clear_history: string
  history_retention_days: number
  max_history_items: number
}

/** @deprecated 后端新架构中不存在此设置 */
export interface AboutSetting {
  version: string
}

// ============================================================================
// 设置上下文接口 - 更新为使用新类型
// ============================================================================

export interface SettingContextType {
  setting: Settings | null
  loading: boolean
  error: string | null
  updateSetting: (newSetting: Settings) => Promise<void>
  updateGeneralSetting: (newGeneralSetting: Partial<GeneralSettings>) => Promise<void>
  updateSyncSetting: (newSyncSetting: Partial<SyncSettings>) => Promise<void>
  updateSecuritySetting: (newSecuritySetting: Partial<SecuritySettings>) => Promise<void>
  updateRetentionPolicy: (newPolicy: Partial<RetentionPolicy>) => Promise<void>
  updateKeyboardShortcuts: (overrides: Record<string, string | string[]>) => Promise<void>
  updateFileSyncSetting: (newFileSyncSetting: Partial<FileSyncSettings>) => Promise<void>
  updateNetworkSetting: (
    newNetworkSetting: Partial<NetworkSettings>
  ) => Promise<{ restartRequired: boolean }>
  updateQuickPanelSetting: (
    newQuickPanelSetting: Partial<QuickPanelSettings>
  ) => Promise<{ restartRequired: boolean }>
}

// ============================================================================
// 导出创建上下文的函数 (保留向后兼容)
// ============================================================================

// SettingContext 在 contexts/SettingContext.tsx 中创建和导出
// 这里只定义类型，不创建 Context 实例以避免循环依赖
