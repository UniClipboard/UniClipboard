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
 *
 * # Transport / 传输 (ADR-008 P6)
 * This module is the SDK exemplar: `getSettings` / `updateSettings` route through
 * the @hey-api generated SDK (`getSettingsSdk` / `updateSettingsSdk`) via
 * `daemonClient.callEnveloped`, which drives the daemon session lifecycle
 * (pre-emptive refresh + one-shot 401 retry) and unwraps the canonical
 * `ApiEnvelope { data, ts }` down to the payload. PUT folds `success` +
 * `restartRequired` INTO that payload (`SettingsUpdateResultDto`). The public
 * wrapper signatures and the hand-written `Settings` domain types below are
 * preserved verbatim for downstream consumers.
 */

import {
  getSettings as getSettingsSdk,
  updateSettings as updateSettingsSdk,
} from '@/api/generated/sdk.gen'
import type { SettingsPatchDto } from '@/api/generated/types.gen'
import { daemonClient } from './client'

// ── Enums ──────────────────────────────────────────────────────

/** Theme preference. / 主题偏好。 */
export type Theme = 'light' | 'dark' | 'system'

/** Quick panel placement. / 快捷面板出现位置。wire: `center` | `follow_cursor`。 */
export type QuickPanelPosition = 'center' | 'follow_cursor'

/** Update channel override. / 更新通道覆盖。 */
export type UpdateChannel = 'stable' | 'alpha' | 'beta' | 'rc'

/** Sync frequency mode. / 同步频率模式。 */
export type SyncFrequency = 'realtime' | 'interval'

/** Retention rule evaluation strategy. / 保留规则评估策略。 */
export type RuleEvaluation = 'anyMatch' | 'allMatch'

// ── Sub-setting interfaces ─────────────────────────────────────

/** General application settings. / 常规应用设置。
 *
 * # themeColor 字段拆分（v0.7+）
 * - `themeColor`: 旧版统一预设字段,daemon 仍透传以兼容老前端,新前端不写入。
 * - `themeColorLight` / `themeColorDark`: 分别是 light / dark 模式下的预设名。
 *   读取时若为 null,daemon 端 `effective_theme_color_*` 会回退到 `themeColor`,
 *   再回退到引擎默认值。
 */
export interface GeneralSettings {
  autoStart: boolean
  silentStart: boolean
  autoCheckUpdate: boolean
  autoDownloadUpdate: boolean
  theme: Theme
  /** 旧版统一主题预设(回退用)。 */
  themeColor: string | null
  /** Light 模式下的主题预设名,如 `"zinc"`、`"catppuccin"`。 */
  themeColorLight: string | null
  /** Dark 模式下的主题预设名,如 `"zinc"`、`"catppuccin"`。 */
  themeColorDark: string | null
  /** Light 模式下用户对预设 token 的自定义覆盖（`{ token: oklchString }`）。 */
  themeOverridesLight: Record<string, string>
  /** Dark 模式下用户对预设 token 的自定义覆盖（语义同 light）。 */
  themeOverridesDark: Record<string, string>
  language: string | null
  deviceName: string | null
  updateChannel?: UpdateChannel | null
  telemetryEnabled: boolean
  usageAnalyticsEnabled: boolean
  debugMode: boolean
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
 * Network settings — wire fields camelCase aligned with daemon serde.
 *
 * 网络设置 — 前端只允许在 NetworkSection.tsx 一处对 `allowRelayFallback` 取反；
 * 不要在前端维护反向布尔镜像字段（反向命名铁律）。
 *
 * `allowOverlayNetworkAddrs` 控制是否把 VPN/overlay 类虚拟网卡 IP（CGNAT
 * 100.64.0.0/10、Tailscale ULA fd7a:115c:a1e0::/48）作为 iroh 直连候选。
 * 默认 `false`。专业用户在两端都接入同一 VPN 时可开启。
 *
 * `customRelayUrls` 为空时沿用 iroh 默认中继；非空时只使用这些自定义
 * relay URL。LAN-only 模式开启时该列表会保留但不生效。
 */
export interface NetworkSettings {
  allowRelayFallback: boolean
  allowOverlayNetworkAddrs: boolean
  customRelayUrls: string[]
}

/**
 * Quick panel (Spotlight-style) feature toggle. / 快捷面板功能开关。
 *
 * Default `enabled = true`. GUI clients toggle this via the
 * `set_quick_panel_enabled` Tauri command. Enabling registers the global
 * shortcut and pre-creates the (hidden) panel window in-process. Disabling
 * unregisters the shortcut immediately, but the hidden webview and its
 * WebContent XPC remain alive until the GUI is restarted — destroying the
 * NSPanel on macOS crashes the process. The daemon HTTP path only persists
 * the flag (used by non-GUI consumers / cross-window sync).
 */
export interface QuickPanelSettings {
  enabled: boolean
  position: QuickPanelPosition
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
  network: NetworkSettings
  quickPanel: QuickPanelSettings
}

// ── API request shape ──────────────────────────────────────────

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
  network?: Partial<NetworkSettings>
  quickPanel?: Partial<QuickPanelSettings>
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
  // Route through the generated SDK; `callEnveloped` unwraps down to the
  // Settings payload. The generated `SettingsDto` is structurally equivalent
  // to the hand-written `Settings` (camelCase wire fields), bridged here to
  // keep the public return type stable for downstream consumers.
  const data = await daemonClient.callEnveloped(() => getSettingsSdk({ throwOnError: true }))
  return data as unknown as Settings
}

/**
 * Apply a partial settings update to the daemon.
 *
 * Only the provided fields are changed; omitted fields retain their current values
 * on the daemon (server-side deep merge).
 *
 * @param settings - Partial settings object containing only the fields to update
 * @returns An object with `success` indicating whether the patch was applied, and
 *          `restartRequired` indicating the daemon requests a restart (e.g., after
 *          certain network-related changes)
 */
export async function updateSettings(
  settings: Partial<Settings>
): Promise<{ success: boolean; restartRequired: boolean }> {
  const patch = toSettingsPatchRequest(settings)
  // Route through the generated SDK. `success` + `restartRequired` are now folded
  // INTO the envelope payload (`SettingsUpdateResultDto`) per ADR-008 §0.1;
  // `callEnveloped` unwraps down to that payload.
  const data = await daemonClient.callEnveloped(() =>
    updateSettingsSdk({
      // `toSettingsPatchRequest` returns a structurally-equivalent camelCase patch;
      // bridge to the generated `SettingsPatchDto` shape for the SDK body param.
      body: patch as unknown as SettingsPatchDto,
      throwOnError: true,
    })
  )
  return { success: data.success, restartRequired: data.restartRequired }
}

/**
 * Constructs a patch object that includes only the top-level settings sections present in the input.
 *
 * @param settings - A partial Settings object; only top-level sections that are defined will be included in the patch.
 * @returns A SettingsPatchRequest containing the provided sections with their corresponding fields.
 */
function toSettingsPatchRequest(settings: Partial<Settings>): SettingsPatchRequest {
  const patch: SettingsPatchRequest = {}

  if (settings.general) {
    // NOTE: `autoStart` is intentionally NOT carried through this patch. The OS
    // launch-at-login registration is a desktop-host side effect that the
    // settings/daemon pipeline does not perform. Autostart is toggled via the
    // dedicated `update_autostart` command (see `@/api/tauri-command/settings`),
    // which persists the preference and applies the OS state atomically.
    const {
      silentStart,
      autoCheckUpdate,
      autoDownloadUpdate,
      theme,
      themeColor,
      themeColorLight,
      themeColorDark,
      themeOverridesLight,
      themeOverridesDark,
      language,
      deviceName,
      updateChannel,
      telemetryEnabled,
      usageAnalyticsEnabled,
      debugMode,
    } = settings.general

    patch.general = {
      silentStart,
      autoCheckUpdate,
      autoDownloadUpdate,
      theme,
      themeColor,
      themeColorLight,
      themeColorDark,
      themeOverridesLight,
      themeOverridesDark,
      language,
      deviceName,
      updateChannel,
      telemetryEnabled,
      usageAnalyticsEnabled,
      debugMode,
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

  if (settings.network) {
    // Spread to only mirror fields actually present on the input — both
    // existing tests and SettingContext sometimes pass `Partial<NetworkSettings>`
    // (e.g. just `{ allowRelayFallback: ... }`); we must not emit undefined
    // fields, since the wire patch interprets `null/undefined` as "no change".
    patch.network = { ...settings.network }
  }

  if (settings.quickPanel) {
    // Same rule as `network`: only emit fields present on the partial input,
    // so undefined-as-no-change wire semantics is preserved.
    patch.quickPanel = { ...settings.quickPanel }
  }

  return patch
}
