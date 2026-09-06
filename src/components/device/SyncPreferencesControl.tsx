import { AlignLeft, FileIcon, ImageIcon, Link2, RotateCcw, Type } from 'lucide-react'
import type { ComponentType } from 'react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes } from '@/api/daemon/member'
import { contentTypeEntries } from '@/components/device/device-utils'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'

type SyncDirection = 'both' | 'send_only' | 'receive_only' | 'off'

const CONTENT_TYPE_ICONS: Partial<
  Record<keyof ContentTypes, ComponentType<{ className?: string }>>
> = {
  text: Type,
  image: ImageIcon,
  file: FileIcon,
  link: Link2,
  richText: AlignLeft,
}

const DIRECTION_VALUES: Record<SyncDirection, { send: boolean; receive: boolean }> = {
  both: { send: true, receive: true },
  send_only: { send: true, receive: false },
  receive_only: { send: false, receive: true },
  off: { send: false, receive: false },
}

interface SyncPreferencesControlProps {
  deviceName: string
  sendEnabled: boolean
  receiveEnabled: boolean
  sendContentTypes?: ContentTypes
  receiveContentTypes?: ContentTypes
  globalSyncOff: boolean
  globalFileSyncOff: boolean
  isLoading: boolean
  onSyncEnabledChange: (checked: boolean) => void
  onContentDirectionChange: (field: keyof ContentTypes, send: boolean, receive: boolean) => void
  onRestoreDefaults: () => void
}

export default function SyncPreferencesControl({
  deviceName,
  sendEnabled,
  receiveEnabled,
  sendContentTypes,
  receiveContentTypes,
  globalSyncOff,
  globalFileSyncOff,
  isLoading,
  onSyncEnabledChange,
  onContentDirectionChange,
  onRestoreDefaults,
}: SyncPreferencesControlProps) {
  const { t } = useTranslation()
  const syncEnabled = sendEnabled || receiveEnabled
  const contentDirections = contentTypeEntries.map(({ field }) =>
    getSyncDirection(
      sendEnabled && (sendContentTypes?.[field] ?? true),
      receiveEnabled && (receiveContentTypes?.[field] ?? true)
    )
  )
  const allBoth = contentDirections.every(direction => direction === 'both')

  return (
    <div
      role="region"
      aria-label={t('devices.settings.sync.title')}
      aria-busy={isLoading}
      className={cn(
        isLoading && '[&_[data-slot=switch]]:invisible [&_[data-slot=select-value]]:invisible'
      )}
    >
      <div className="px-5 py-5 @md:px-6">
        <div className="flex items-center justify-between gap-5">
          <div className="min-w-0">
            <h4 className="truncate text-sm font-semibold text-foreground">
              {t('devices.settings.sync.syncWithDevice', { deviceName })}
            </h4>
            <p className={cn('mt-1 text-xs text-muted-foreground', isLoading && 'invisible')}>
              {t(
                syncEnabled
                  ? 'devices.settings.sync.enabledDescription'
                  : 'devices.settings.sync.disabledDescription'
              )}
            </p>
          </div>
          <Switch
            aria-label={t('devices.settings.sync.syncWithDevice', { deviceName })}
            checked={syncEnabled}
            disabled={globalSyncOff || isLoading}
            onCheckedChange={onSyncEnabledChange}
          />
        </div>
        {globalSyncOff && (
          <p role="status" className="mt-4 rounded-lg border bg-muted/40 p-3 text-sm">
            {t('devices.thisDevice.syncPaused')}
          </p>
        )}
      </div>

      <div className="flex w-full items-center justify-between gap-4 border-t border-border/50 px-5 py-4 @md:px-6">
        <span className="text-sm font-medium text-foreground">
          {t('devices.settings.sync.customize')}
        </span>
        <span className={cn('truncate text-xs text-muted-foreground', isLoading && 'invisible')}>
          {t(
            syncEnabled && allBoth
              ? 'devices.settings.sync.allBoth'
              : 'devices.settings.sync.customized'
          )}
        </span>
      </div>

      <div className="border-t border-border/50">
        <div className="flex items-center justify-between gap-4 bg-muted/20 px-5 py-3 @md:px-6">
          <p className="text-xs text-muted-foreground">
            {t('devices.settings.sync.customizeDescription')}
          </p>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={t('devices.settings.sync.restoreDefaults')}
            title={t('devices.settings.sync.restoreDefaults')}
            onClick={onRestoreDefaults}
            disabled={globalSyncOff || isLoading}
            className="shrink-0 text-muted-foreground hover:text-foreground"
          >
            <RotateCcw className="size-3.5" />
            <span className="hidden text-xs @sm:inline">
              {t('devices.settings.sync.restoreDefaults')}
            </span>
          </Button>
        </div>

        <div>
          {contentTypeEntries.map(({ field, status }) => {
            const Icon = CONTENT_TYPE_ICONS[field]!
            const label = t(`devices.settings.sync.typeLabels.${field}`)
            const unavailable = status === 'coming_soon'
            const fileSyncOff = field === 'file' && globalFileSyncOff
            const disabled =
              !syncEnabled || globalSyncOff || fileSyncOff || unavailable || isLoading
            const direction = getSyncDirection(
              sendEnabled && (sendContentTypes?.[field] ?? true),
              receiveEnabled && (receiveContentTypes?.[field] ?? true)
            )

            return (
              <div
                key={field}
                className="flex min-h-14 items-center justify-between gap-4 border-t border-border/35 px-5 py-3 first:border-t-0 @md:px-6"
              >
                <div className="flex min-w-0 items-center gap-3">
                  <Icon className="size-4 shrink-0 text-muted-foreground" />
                  <span className="min-w-0">
                    <span className="block text-sm text-foreground">{label}</span>
                    {fileSyncOff && (
                      <Badge
                        variant="outline"
                        className="mt-1 max-w-full border-warning/20 bg-warning/10 px-1.5 py-0 text-[9px] leading-4 text-warning"
                      >
                        {t('devices.settings.badges.globalFileSyncOff')}
                      </Badge>
                    )}
                    {unavailable && (
                      <Badge variant="secondary" className="mt-1 px-1.5 py-0 text-[9px] leading-4">
                        {t('devices.settings.badges.comingSoon')}
                      </Badge>
                    )}
                  </span>
                </div>

                <Select
                  value={direction}
                  disabled={disabled}
                  onValueChange={(value: SyncDirection) => {
                    const next = DIRECTION_VALUES[value]
                    onContentDirectionChange(field, next.send, next.receive)
                  }}
                >
                  <SelectTrigger
                    size="sm"
                    aria-label={t('devices.settings.sync.directionLabel', {
                      contentType: label,
                    })}
                    className={cn('w-44 max-w-[55%]', disabled && 'bg-muted/20')}
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent align="end" className="w-max min-w-52 max-w-72">
                    <SelectItem value="both">{t('devices.settings.sync.modes.both')}</SelectItem>
                    <SelectItem value="send_only">
                      {t('devices.settings.sync.modes.sendOnly', { deviceName })}
                    </SelectItem>
                    <SelectItem value="receive_only">
                      {t('devices.settings.sync.modes.receiveOnly', { deviceName })}
                    </SelectItem>
                    <SelectItem value="off">{t('devices.settings.sync.modes.off')}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

function getSyncDirection(send: boolean, receive: boolean): SyncDirection {
  if (send && receive) return 'both'
  if (send) return 'send_only'
  if (receive) return 'receive_only'
  return 'off'
}
