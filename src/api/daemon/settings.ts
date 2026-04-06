/**
 * Settings API module — typed accessors for daemon settings endpoints.
 *
 * 设置 API 模块 — daemon 设置端点的类型化访问器。
 *
 * # Endpoints / 端点
 * - `GET /settings` → current application settings
 * - `PUT /settings` → partial-update (deep merge on server side)
 *
 * # Note / 注意
 * Unlike the Tauri command, the daemon HTTP settings endpoint does NOT apply
 * OS-level side effects (autostart registration, keyboard shortcut updates).
 * It only persists the settings domain model.
 */

import { daemonClient } from './client'

// ── Enums ──────────────────────────────────────────────────────

/** Theme preference. / 主题偏好。 */
export type Theme = 'light' | 'dark' | 'system'

/** Update channel override. / 更新通道覆盖。 */
export type UpdateChannel = 'stable' | 'alpha' | 'beta' | 'rc'

/** Sync frequency mode. / 同步频率模式。 */
export type SyncFrequency = 'realtime' | 'interval'

/** Retention rule evaluation strategy. / 保留规则评估策略。 */
export type RuleEvaluation = 'anyMatch' | 'allMatch'

// ── Sub-setting interfaces ─────────────────────────────────────

/** General application settings. / 常规应用设置。 */
export interface GeneralSettings {
  autoStart: boolean
  silentStart: boolean
  autoCheckUpdate: boolean
  theme: Theme
  themeColor: string | null
  language: string | null
  deviceName: string | null
  updateChannel?: UpdateChannel | null
}

/** Content type toggles for sync filtering. / 同步过滤的内容类型开关。 */
export interface ContentTypes {
  text: boolean
  image: boolean
  link: boolean
  file: boolean
  codeSnippet: boolean
  richText: boolean
}

/** Sync behaviour settings. / 同步行为设置。 */
export interface SyncSettings {
  autoSync: boolean
  syncFrequency: SyncFrequency
  contentTypes: ContentTypes
}

/** Security / encryption settings. / 安全/加密设置。 */
export interface SecuritySettings {
  encryptionEnabled: boolean
  passphraseConfigured: boolean
  autoUnlockEnabled: boolean
}

/** Pairing timeout and protocol settings. / 配对超时和协议设置。 */
export interface PairingSettings {
  /** Step timeout in seconds. */
  stepTimeout: number
  /** User verification timeout in seconds. */
  userVerificationTimeout: number
  /** Session timeout in seconds. */
  sessionTimeout: number
  maxRetries: number
  protocolVersion: string
}

/** File sync settings. / 文件同步设置。 */
export interface FileSyncSettings {
  fileSyncEnabled: boolean
  smallFileThreshold: number
  maxFileSize: number
  fileCacheQuotaPerDevice: number
  fileRetentionHours: number
  fileAutoCleanup: boolean
}

/**
 * Retention rule — discriminated union matching the Rust `RetentionRule` enum.
 *
 * 保留规则 — 与 Rust `RetentionRule` 枚举匹配的可区分联合类型。
 */
export type RetentionRule =
  | { byAge: { maxAge: number } }
  | { byCount: { maxItems: number } }
  | { byContentType: { contentType: ContentTypes; maxAge: number } }
  | { byTotalSize: { maxBytes: number } }
  | { sensitive: { maxAge: number } }

/** Retention policy configuration. / 保留策略配置。 */
export interface RetentionPolicy {
  enabled: boolean
  rules: RetentionRule[]
  skipPinned: boolean
  evaluation: RuleEvaluation
}

/**
 * Keyboard shortcut value — single key combo or multiple alternatives.
 *
 * 键盘快捷键值 — 单个按键组合或多个备选方案。
 */
export type ShortcutKey = string | string[]

// ── Top-level Settings ─────────────────────────────────────────

/**
 * Complete application settings matching `uc-core::settings::model::Settings`.
 *
 * 完整应用设置，匹配 `uc-core::settings::model::Settings`。
 *
 * Field names are camelCase to match the Rust serde serialisation.
 */
export interface Settings {
  schemaVersion: number
  general: GeneralSettings
  sync: SyncSettings
  retentionPolicy: RetentionPolicy
  security: SecuritySettings
  pairing: PairingSettings
  keyboardShortcuts: Record<string, ShortcutKey>
  fileSync: FileSyncSettings
}

// ── API response wrappers ──────────────────────────────────────

/** GET /settings response shape. / GET /settings 响应结构。 */
interface SettingsGetResponse {
  data: Settings
  ts: number
}

/** PUT /settings response shape. / PUT /settings 响应结构。 */
interface SettingsUpdateResponse {
  data: { success: boolean }
  ts: number
}

interface SettingsPatchRequest {
  general?: Partial<GeneralSettings>
  sync?: Partial<SyncSettings>
  retentionPolicy?: Partial<RetentionPolicy>
  security?: {
    encryptionEnabled?: boolean
    autoUnlockEnabled?: boolean
    passphrase?: string
  }
  pairing?: {
    stepTimeout?: number
    userVerificationTimeout?: number
    sessionTimeout?: number
    maxRetries?: number
  }
  keyboardShortcuts?: {
    shortcuts: Record<string, ShortcutKey>
  }
  fileSync?: Partial<FileSyncSettings>
}

// ── Public API ─────────────────────────────────────────────────

/**
 * Fetch the current application settings from the daemon.
 *
 * 从 daemon 获取当前应用设置。
 *
 * @returns The full Settings object.
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function getSettings(): Promise<Settings> {
  const res = await daemonClient.request<SettingsGetResponse>('/settings')
  return res.data
}

/**
 * Update application settings via deep merge on the server.
 *
 * 通过服务器端深度合并更新应用设置。
 *
 * Only the provided fields are changed; omitted fields retain their current
 * values. Nested objects are merged recursively.
 *
 * @param settings Partial settings payload.
 * @throws {DaemonApiError} On HTTP or validation errors.
 */
export async function updateSettings(settings: Partial<Settings>): Promise<void> {
  const patch = toSettingsPatchRequest(settings)
  await daemonClient.request<SettingsUpdateResponse>('/settings', {
    method: 'PUT',
    body: patch,
  })
}

function toSettingsPatchRequest(settings: Partial<Settings>): SettingsPatchRequest {
  const patch: SettingsPatchRequest = {}

  if (settings.general) {
    const {
      autoStart,
      silentStart,
      autoCheckUpdate,
      theme,
      themeColor,
      language,
      deviceName,
      updateChannel,
      telemetryEnabled,
    } = settings.general

    patch.general = {
      autoStart,
      silentStart,
      autoCheckUpdate,
      theme,
      themeColor,
      language,
      deviceName,
      updateChannel,
      telemetryEnabled,
    }
  }

  if (settings.sync) {
    const { autoSync, syncFrequency, contentTypes } = settings.sync
    patch.sync = {
      autoSync,
      syncFrequency,
      contentTypes,
    }
  }

  if (settings.retentionPolicy) {
    const { enabled, rules, skipPinned, evaluation } = settings.retentionPolicy
    patch.retentionPolicy = {
      enabled,
      rules,
      skipPinned,
      evaluation,
    }
  }

  if (settings.security) {
    const { encryptionEnabled, autoUnlockEnabled } = settings.security
    patch.security = {
      encryptionEnabled,
      autoUnlockEnabled,
    }
  }

  if (settings.pairing) {
    const { stepTimeout, userVerificationTimeout, sessionTimeout, maxRetries } = settings.pairing
    patch.pairing = {
      stepTimeout,
      userVerificationTimeout,
      sessionTimeout,
      maxRetries,
    }
  }

  if (settings.keyboardShortcuts) {
    patch.keyboardShortcuts = {
      shortcuts: settings.keyboardShortcuts,
    }
  }

  if (settings.fileSync) {
    const {
      fileSyncEnabled,
      smallFileThreshold,
      maxFileSize,
      fileCacheQuotaPerDevice,
      fileRetentionHours,
      fileAutoCleanup,
    } = settings.fileSync
    patch.fileSync = {
      fileSyncEnabled,
      smallFileThreshold,
      maxFileSize,
      fileCacheQuotaPerDevice,
      fileRetentionHours,
      fileAutoCleanup,
    }
  }

  return patch
}
