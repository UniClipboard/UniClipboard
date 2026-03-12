// ============================================================================
// 新架构类型定义 - 与 Rust 后端 uc-core/src/settings/model.rs 完全匹配
// ============================================================================

/**
 * 主题模式 - 对应 Rust Theme enum
 * serde(rename_all = "snake_case") 会将枚举序列化为小写下划线形式
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
  auto_start: boolean
  silent_start: boolean
  auto_check_update: boolean
  theme: Theme
  theme_color?: string | null
  language?: string | null
  device_name?: string | null
  update_channel?: UpdateChannel | null
}

/**
 * 内容类型 - 对应 Rust ContentTypes
 */
export interface ContentTypes {
  text: boolean
  image: boolean
  link: boolean
  file: boolean
  code_snippet: boolean
  rich_text: boolean
}

/**
 * 同步频率 - 对应 Rust SyncFrequency enum
 */
export type SyncFrequency = 'realtime' | 'interval'

/**
 * 同步设置 - 对应 Rust SyncSettings
 */
export interface SyncSettings {
  auto_sync: boolean
  sync_frequency: SyncFrequency
  content_types: ContentTypes
  max_file_size_mb: number
}

/**
 * 持续时间表示 - 对应 Rust Duration
 * Rust serde_with::DurationSeconds<u64> 将 Duration 序列化为秒数
 */
export type DurationSeconds = number

/**
 * 保留规则 - 对应 Rust RetentionRule enum
 * Rust 使用 serde externally-tagged + rename_all="snake_case"
 * 序列化为 { "by_age": { "max_age": 2592000 } } 格式
 */
export type RetentionRule =
  | { by_age: { max_age: DurationSeconds } }
  | { by_count: { max_items: number } }
  | { by_content_type: { content_type: ContentTypes; max_age: DurationSeconds } }
  | { by_total_size: { max_bytes: number } }
  | { sensitive: { max_age: DurationSeconds } }

/**
 * 规则评估方式 - 对应 Rust RuleEvaluation enum
 */
export type RuleEvaluation = 'any_match' | 'all_match'

/**
 * 保留策略 - 对应 Rust RetentionPolicy
 */
export interface RetentionPolicy {
  enabled: boolean
  rules: RetentionRule[]
  skip_pinned: boolean
  evaluation: RuleEvaluation
}

/**
 * 安全设置 - 对应 Rust SecuritySettings
 */
export interface SecuritySettings {
  encryption_enabled: boolean
  passphrase_configured: boolean
  auto_unlock_enabled: boolean
}

/**
 * 配对设置 - 对应 Rust PairingSettings
 */
export interface PairingSettings {
  step_timeout: DurationSeconds
  user_verification_timeout: DurationSeconds
  session_timeout: DurationSeconds
  max_retries: number
  protocol_version: string
}

/**
 * 应用设置 - 对应 Rust Settings
 */
export interface Settings {
  schema_version: number
  general: GeneralSettings
  sync: SyncSettings
  retention_policy: RetentionPolicy
  security: SecuritySettings
  pairing: PairingSettings
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
}

// ============================================================================
// 导出创建上下文的函数 (保留向后兼容)
// ============================================================================

// SettingContext 在 contexts/SettingContext.tsx 中创建和导出
// 这里只定义类型，不创建 Context 实例以避免循环依赖
