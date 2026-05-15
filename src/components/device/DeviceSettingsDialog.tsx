/**
 * DeviceSettingsDialog —— 单台 P2P 配对设备的设置 modal。
 *
 * 视觉与 V2 原型 `PeerSettingsDialog` 一致：
 *
 *   ┌────────────────────────────────────────────────────────────┐
 *   │ ⬛ 设备名                                                   │
 *   │   ● 在线  ·  mp_…h7h8                                       │
 *   ├─ 连接信息 ─────────────────────────────────────────────────┤
 *   │ ┌──────────────────────────────────────────────────────┐   │
 *   │ │ 通道                                          直连    │   │
 *   │ └──────────────────────────────────────────────────────┘   │
 *   ├─ 同步设置 ────────────────────────────────────  恢复默认   │
 *   │ ┌─ 同步到此设备 ─────────────────────────────────  [●] ┐  │
 *   │ │   关闭后将不再向此设备同步任何内容                    │  │
 *   │ └────────────────────────────────────────────────────┘   │
 *   │  内容类型                                                  │
 *   │ ┌── 文本 [●] ──┐ ┌── 图片 [●] ──┐                        │
 *   │ ┌── 文件 [●] ──┐ ┌── 链接 [●] ──┐                        │
 *   │ ┌── 富文本 [●] ──┐                                       │
 *   ├────────────────────────────────────────────────────────────┤
 *   │ [取消配对]                                       [完成]    │
 *   └────────────────────────────────────────────────────────────┘
 *
 * 与原右侧抽屉的差别：
 *   - 信息块改 InfoRow 风格圆角 card，不再用 ALL CAPS list section
 *   - 内容类型从单列 SettingRow 改 2-col pill toggle 网格
 *   - 头部状态从 Badge 改 dot-pill，跟 Hero 一致
 *   - 恢复默认从 footer 移到 "同步设置" 区头的 ghost 按钮，footer 只保留
 *     破坏性动作 + 完成
 *
 * 备注名（rename peer）暂未在原型中实装：后端目前无 rename API，先省略；
 * 加 API 时把第三个 DialogSection 接上 controlled Input + 提交即可。
 */

import { AlignLeft, FileIcon, ImageIcon, Link2, Type, type LucideIcon } from 'lucide-react'
import React, { useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes } from '@/api/daemon/member'
import { DEFAULT_SEND_CONTENT_TYPES } from '@/api/daemon/member'
import type { SpaceMember } from '@/api/daemon/members'
import { deriveBadgeKind } from '@/components/device/ConnectionChannelBadge'
import { contentTypeEntries, getDeviceIcon } from '@/components/device/device-utils'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { cn, formatPeerIdForDisplay } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  fetchMemberSyncPreferences,
  updateMemberSyncPreferences,
} from '@/store/slices/devicesSlice'

interface DeviceSettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceId: string
  device: SpaceMember | undefined
  globalAutoSyncOff: boolean
  globalFileSyncOff: boolean
  /** LAN-only Mode 是否已开启（用于派生 channel 显示文案）。 */
  lanOnlyActive: boolean
  onUnpair: (peerId: string) => void
}

const CONTENT_TYPE_ICONS: Partial<Record<keyof ContentTypes, LucideIcon>> = {
  text: Type,
  image: ImageIcon,
  file: FileIcon,
  link: Link2,
  richText: AlignLeft,
}

const DeviceSettingsDialog: React.FC<DeviceSettingsDialogProps> = ({
  open,
  onOpenChange,
  deviceId,
  device,
  globalAutoSyncOff,
  globalFileSyncOff,
  lanOnlyActive,
  onUnpair,
}) => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  const preferences = useAppSelector(state => state.devices.memberSyncPreferences[deviceId])
  const isLoading = useAppSelector(
    state => state.devices.memberSyncPreferencesLoading[deviceId] ?? false
  )

  useEffect(() => {
    if (open && deviceId) {
      dispatch(fetchMemberSyncPreferences(deviceId))
    }
  }, [dispatch, deviceId, open])

  const handleSendEnabledToggle = useCallback(
    (checked: boolean) => {
      dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: { sendEnabled: checked },
        })
      )
    },
    [dispatch, deviceId]
  )

  const handleSendContentTypeToggle = useCallback(
    (field: keyof ContentTypes, checked: boolean) => {
      dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: { sendContentTypes: { [field]: checked } },
        })
      )
    },
    [dispatch, deviceId]
  )

  const handleRestoreDefaults = useCallback(async () => {
    // Phase 4b PR-3：UX 只露 send 列,所以 restore 仅重置 send 字段。
    // receive 字段保留服务端当前值（新 admit 的成员默认就是 true + 全开）。
    await dispatch(
      updateMemberSyncPreferences({
        deviceId,
        patch: {
          sendEnabled: true,
          sendContentTypes: DEFAULT_SEND_CONTENT_TYPES,
        },
      })
    )
    dispatch(fetchMemberSyncPreferences(deviceId))
  }, [dispatch, deviceId])

  if (!device && !deviceId) return null

  const deviceName = device?.deviceName || t('devices.list.labels.unknownDevice')
  const connected = device?.connected ?? false
  const channelKind = deriveBadgeKind(device?.channel ?? 'unknown', lanOnlyActive)
  const channelLabel = t(`devices.list.channel.${channelKind}`)
  const sendEnabled = preferences?.sendEnabled ?? true
  const sendDisabled = !sendEnabled || globalAutoSyncOff || isLoading
  const showSkeleton = isLoading && !preferences

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-3">
            <div
              className={cn(
                'flex h-11 w-11 shrink-0 items-center justify-center rounded-xl',
                connected
                  ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                  : 'bg-muted text-muted-foreground'
              )}
            >
              {React.createElement(getDeviceIcon(device?.deviceName), { className: 'h-5 w-5' })}
            </div>
            <div className="min-w-0 flex-1">
              <DialogTitle className="truncate text-left">{deviceName}</DialogTitle>
              <p className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
                <span
                  className={cn(
                    'inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium',
                    connected
                      ? 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
                      : 'bg-muted text-muted-foreground'
                  )}
                >
                  <span
                    className={cn(
                      'h-1.5 w-1.5 rounded-full',
                      connected ? 'bg-emerald-500' : 'bg-muted-foreground/50'
                    )}
                  />
                  {connected ? t('devices.list.status.online') : t('devices.list.status.offline')}
                </span>
                <span className="truncate font-mono">{formatPeerIdForDisplay(device?.peerId)}</span>
              </p>
            </div>
          </div>
        </DialogHeader>

        <div className="space-y-5">
          {/* ── 连接信息 ──────────────────────────────────────── */}
          <DialogSection
            title={t('devices.settings.sections.connection', { defaultValue: '连接信息' })}
          >
            <InfoRow
              label={t('devices.settings.fields.channel', { defaultValue: '通道' })}
              value={channelLabel}
            />
            {device?.connectionAddress && (
              <InfoRow
                label={t('devices.settings.fields.address', { defaultValue: '地址' })}
                value={device.connectionAddress}
                mono
              />
            )}
          </DialogSection>

          {/* ── 同步设置 ──────────────────────────────────────── */}
          <DialogSection
            title={t('devices.settings.sync.title')}
            trailing={
              <button
                type="button"
                onClick={handleRestoreDefaults}
                disabled={globalAutoSyncOff || isLoading}
                className="text-[11px] font-medium text-muted-foreground transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
              >
                {t('devices.settings.sync.restoreDefaults')}
              </button>
            }
          >
            {showSkeleton ? (
              <SyncSettingsSkeleton />
            ) : (
              <>
                <SettingToggleRow
                  label={t('devices.settings.sync.rules.sendEnabled.title')}
                  description={t('devices.settings.sync.rules.sendEnabled.description')}
                  checked={sendEnabled}
                  onChange={handleSendEnabledToggle}
                  disabled={globalAutoSyncOff || isLoading}
                />

                <div className="space-y-2">
                  <p className="px-1 text-[11px] uppercase tracking-wider text-muted-foreground">
                    {t('devices.settings.sync.contentTypes', { defaultValue: '内容类型' })}
                  </p>
                  <div
                    className={cn(
                      'grid grid-cols-2 gap-2 transition-opacity',
                      sendDisabled && 'pointer-events-none opacity-50'
                    )}
                  >
                    {contentTypeEntries.map(({ field, i18nKey, status }) => {
                      const isComingSoon = status === 'coming_soon'
                      const isGlobalFileSyncDisabled = field === 'file' && globalFileSyncOff
                      const disabled = isComingSoon || isGlobalFileSyncDisabled || sendDisabled

                      let suffix: React.ReactNode = null
                      if (isComingSoon) {
                        suffix = (
                          <Badge variant="secondary" className="px-1.5 py-0 text-[9px]">
                            {t('devices.settings.badges.comingSoon')}
                          </Badge>
                        )
                      } else if (isGlobalFileSyncDisabled) {
                        suffix = (
                          <Badge
                            variant="outline"
                            className="border-amber-500/20 bg-amber-500/10 px-1.5 py-0 text-[9px] text-amber-600 dark:text-amber-400"
                          >
                            {t('devices.settings.badges.globalFileSyncOff')}
                          </Badge>
                        )
                      }

                      return (
                        <ContentTypeToggle
                          key={field}
                          // `contentTypeEntries` 已剔除 codeSnippet, `field` 只会落到
                          // 有映射的 5 种 key, 故此处用 non-null 断言绕过 Partial 索引签名。
                          icon={CONTENT_TYPE_ICONS[field]!}
                          label={t(`devices.settings.sync.rules.${i18nKey}.title`)}
                          checked={preferences?.sendContentTypes[field] ?? true}
                          onChange={checked => handleSendContentTypeToggle(field, checked)}
                          disabled={disabled}
                          suffix={suffix}
                        />
                      )
                    })}
                  </div>
                </div>
              </>
            )}
          </DialogSection>
        </div>

        <DialogFooter className="!flex-row !justify-between">
          <Button variant="destructive" size="sm" onClick={() => onUnpair(deviceId)}>
            {t('devices.list.actions.unpair')}
          </Button>
          <Button size="sm" onClick={() => onOpenChange(false)}>
            {t('devices.addDevice.actions.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default DeviceSettingsDialog

// ────────────────────────────────────────────────────────────────
// Section / row helpers (本文件局部，不外露)
// ────────────────────────────────────────────────────────────────

const DialogSection: React.FC<{
  title: string
  trailing?: React.ReactNode
  children: React.ReactNode
}> = ({ title, trailing, children }) => (
  <section className="space-y-2">
    <div className="flex items-center justify-between px-1">
      <h5 className="text-[11px] uppercase tracking-wider text-muted-foreground">{title}</h5>
      {trailing}
    </div>
    <div className="space-y-2">{children}</div>
  </section>
)

const InfoRow: React.FC<{ label: string; value: string; mono?: boolean }> = ({
  label,
  value,
  mono,
}) => (
  <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/50 px-3 py-2 text-xs">
    <span className="shrink-0 text-muted-foreground">{label}</span>
    <span className={cn('min-w-0 truncate text-foreground', mono && 'font-mono')} title={value}>
      {value}
    </span>
  </div>
)

const SettingToggleRow: React.FC<{
  label: string
  description?: string
  checked: boolean
  disabled?: boolean
  onChange: (v: boolean) => void
}> = ({ label, description, checked, disabled, onChange }) => (
  <div className="flex items-start justify-between gap-3 rounded-lg border border-border/60 bg-card/50 px-3 py-2.5">
    <div className="min-w-0 flex-1">
      <p className="text-sm font-medium text-foreground">{label}</p>
      {description && (
        <p className="mt-0.5 text-[11px] leading-snug text-muted-foreground">{description}</p>
      )}
    </div>
    <Switch checked={checked} onCheckedChange={onChange} disabled={disabled} />
  </div>
)

const ContentTypeToggle: React.FC<{
  icon: LucideIcon
  label: string
  checked: boolean
  disabled?: boolean
  suffix?: React.ReactNode
  onChange: (v: boolean) => void
}> = ({ icon: Icon, label, checked, disabled, suffix, onChange }) => (
  <label
    className={cn(
      'flex items-center gap-2 rounded-lg border px-3 py-2 transition-colors',
      disabled
        ? 'cursor-not-allowed border-border/60 bg-card/30'
        : checked
          ? 'cursor-pointer border-primary/40 bg-primary/5'
          : 'cursor-pointer border-border/60 bg-card/50 hover:bg-muted/30'
    )}
  >
    <Icon
      className={cn(
        'h-3.5 w-3.5 shrink-0',
        checked && !disabled ? 'text-primary' : 'text-muted-foreground'
      )}
    />
    <span className="flex-1 truncate text-xs font-medium text-foreground">{label}</span>
    {suffix}
    <Switch size="sm" checked={checked} onCheckedChange={onChange} disabled={disabled} />
  </label>
)

const SyncSettingsSkeleton: React.FC = () => (
  <>
    <Skeleton className="h-[58px] w-full rounded-lg" />
    <div className="space-y-2">
      <Skeleton className="h-3 w-16 rounded" />
      <div className="grid grid-cols-2 gap-2">
        {[0, 1, 2, 3].map(i => (
          <Skeleton key={i} className="h-[34px] rounded-lg" />
        ))}
      </div>
    </div>
  </>
)
