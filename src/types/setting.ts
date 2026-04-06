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
 */
export interface GeneralSettings {
  autoStart: boolean
  silentStart: boolean
  autoCheckUpdate: boolean
  theme: Theme
  themeColor: string | null
  language: string | null
  deviceName: string | null
  updateChannel?: UpdateChannel | null
  telemetryEnabled: boolean
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
}

// ============================================================================
// 导出创建上下文的函数 (保留向后兼容)
// ============================================================================

// SettingContext 在 contexts/SettingContext.tsx 中创建和导出
// 这里只定义类型，不创建 Context 实例以避免循环依赖
