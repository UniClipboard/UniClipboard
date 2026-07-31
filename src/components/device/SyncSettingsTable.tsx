import { AlignLeft, FileIcon, ImageIcon, Link2, Type, type LucideIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes } from '@/api/daemon/member'
import { contentTypeEntries } from '@/components/device/device-utils'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'

const CONTENT_TYPE_PRESENTATION: Partial<
  Record<keyof ContentTypes, { icon: LucideIcon; color: string }>
> = {
  text: { icon: Type, color: 'text-sky-500 dark:text-sky-400' },
  image: { icon: ImageIcon, color: 'text-rose-500 dark:text-rose-400' },
  file: { icon: FileIcon, color: 'text-amber-500 dark:text-amber-400' },
  link: { icon: Link2, color: 'text-teal-500 dark:text-teal-400' },
  richText: { icon: AlignLeft, color: 'text-violet-500 dark:text-violet-400' },
}

interface SyncSettingsTableProps {
  sendEnabled: boolean
  receiveEnabled: boolean
  sendContentTypes?: ContentTypes
  receiveContentTypes?: ContentTypes
  globalAutoSyncOff: boolean
  globalFileSyncOff: boolean
  isLoading: boolean
  onSendEnabledChange: (checked: boolean) => void
  onReceiveEnabledChange: (checked: boolean) => void
  onSendContentTypeChange: (field: keyof ContentTypes, checked: boolean) => void
  onReceiveContentTypeChange: (field: keyof ContentTypes, checked: boolean) => void
}

export default function SyncSettingsTable({
  sendEnabled,
  receiveEnabled,
  sendContentTypes,
  receiveContentTypes,
  globalAutoSyncOff,
  globalFileSyncOff,
  isLoading,
  onSendEnabledChange,
  onReceiveEnabledChange,
  onSendContentTypeChange,
  onReceiveContentTypeChange,
}: SyncSettingsTableProps) {
  const { t } = useTranslation()
  const sendControlsDisabled = !sendEnabled || globalAutoSyncOff || isLoading
  const receiveControlsDisabled = !receiveEnabled || globalAutoSyncOff || isLoading
  const sendLabel = t('devices.settings.sync.columns.send')
  const receiveLabel = t('devices.settings.sync.columns.receive')

  return (
    <div className="mt-2.5 overflow-hidden rounded-lg border border-border/45 bg-background">
      <table
        aria-label={t('devices.settings.sync.title')}
        className="w-full table-fixed border-collapse"
      >
        <colgroup>
          <col className="w-1/2" />
          <col className="w-1/4" />
          <col className="w-1/4" />
        </colgroup>
        <thead>
          <tr className="border-b border-border/45">
            <th
              scope="col"
              className="h-11 px-4 text-left text-[11px] font-medium text-muted-foreground"
            >
              {t('devices.settings.sync.contentTypes')}
            </th>
            <th scope="col" className="h-11 bg-muted/15 px-3">
              <div className="flex items-center justify-center gap-2">
                <span className="text-[11px] font-semibold text-foreground">{sendLabel}</span>
                <Switch
                  size="sm"
                  aria-label={t('devices.settings.sync.rules.sendEnabled.title')}
                  checked={sendEnabled}
                  disabled={globalAutoSyncOff || isLoading}
                  onCheckedChange={onSendEnabledChange}
                />
              </div>
            </th>
            <th scope="col" className="h-11 bg-muted/15 px-3">
              <div className="flex items-center justify-center gap-2">
                <span className="text-[11px] font-semibold text-foreground">{receiveLabel}</span>
                <Switch
                  size="sm"
                  aria-label={t('devices.settings.sync.rules.receiveEnabled.title')}
                  checked={receiveEnabled}
                  disabled={globalAutoSyncOff || isLoading}
                  onCheckedChange={onReceiveEnabledChange}
                />
              </div>
            </th>
          </tr>
        </thead>
        <tbody>
          {contentTypeEntries.map(({ field, status }) => {
            const { icon: Icon, color } = CONTENT_TYPE_PRESENTATION[field]!
            const label = t(`devices.settings.sync.typeLabels.${field}`)
            const unavailable = status === 'coming_soon'
            const fileSyncOff = field === 'file' && globalFileSyncOff
            const sendDisabled = unavailable || fileSyncOff || sendControlsDisabled
            const receiveDisabled = unavailable || fileSyncOff || receiveControlsDisabled

            return (
              <tr key={field} className="h-11 transition-colors hover:bg-muted/20">
                <th scope="row" className="px-4 py-2 text-left font-normal">
                  <div className="flex min-w-0 items-center gap-3">
                    <Icon className={cn('size-4 shrink-0', color)} />
                    <span className="min-w-0">
                      <span className="block text-[12.5px] font-medium leading-tight text-foreground">
                        {label}
                      </span>
                      {fileSyncOff && (
                        <Badge
                          variant="outline"
                          className="mt-1 max-w-full border-warning/20 bg-warning/10 px-1.5 py-0 text-[9px] leading-4 text-warning"
                        >
                          {t('devices.settings.badges.globalFileSyncOff')}
                        </Badge>
                      )}
                      {unavailable && (
                        <Badge
                          variant="secondary"
                          className="mt-1 px-1.5 py-0 text-[9px] leading-4"
                        >
                          {t('devices.settings.badges.comingSoon')}
                        </Badge>
                      )}
                    </span>
                  </div>
                </th>
                <td
                  className={cn(
                    'bg-muted/[0.06] px-3 text-center transition-colors',
                    sendControlsDisabled && 'bg-muted/20'
                  )}
                >
                  <Switch
                    size="sm"
                    aria-label={t('devices.settings.sync.toggleLabel', {
                      direction: sendLabel,
                      contentType: label,
                    })}
                    className={cn('transition-opacity', sendControlsDisabled && 'opacity-40')}
                    checked={sendContentTypes?.[field] ?? true}
                    disabled={sendDisabled}
                    onCheckedChange={checked => onSendContentTypeChange(field, checked)}
                  />
                </td>
                <td
                  className={cn(
                    'bg-muted/[0.06] px-3 text-center transition-colors',
                    receiveControlsDisabled && 'bg-muted/20'
                  )}
                >
                  <Switch
                    size="sm"
                    aria-label={t('devices.settings.sync.toggleLabel', {
                      direction: receiveLabel,
                      contentType: label,
                    })}
                    className={cn('transition-opacity', receiveControlsDisabled && 'opacity-40')}
                    checked={receiveContentTypes?.[field] ?? true}
                    disabled={receiveDisabled}
                    onCheckedChange={checked => onReceiveContentTypeChange(field, checked)}
                  />
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
