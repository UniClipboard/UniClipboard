import { useTranslation } from 'react-i18next'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { Button } from '@/components/ui'
import { cn } from '@/lib/utils'
import type { RelayOverview, RelayOverviewEntry } from '@/types/setting'

const KNOWN_REGION_IDS = ['na-east', 'na-west', 'eu', 'asia-pacific'] as const

function regionLabelKey(regionId: string | null): string | null {
  return regionId && (KNOWN_REGION_IDS as readonly string[]).includes(regionId)
    ? `settings.sections.network.builtInRelays.regions.${regionId}`
    : null
}

interface BuiltInRelaysFieldProps {
  overview: RelayOverview | null
  loading: boolean
  failed: boolean
  onRetry: () => void
}

/**
 * Read-only list of the built-in relays plus one line describing the routing
 * state. `inEffect` only says the running node was configured with the relay;
 * it is never presented as a connection state.
 */
export function BuiltInRelaysField({
  overview,
  loading,
  failed,
  onRetry,
}: BuiltInRelaysFieldProps) {
  const { t } = useTranslation()
  const title = t('settings.sections.network.groups.builtInRelays')

  if (failed) {
    return (
      <SettingGroup title={title}>
        <div className="mt-3 rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-ui-body text-destructive">
          <p>{t('settings.sections.network.builtInRelays.loadError')}</p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="mt-2"
            disabled={loading}
            onClick={onRetry}
          >
            {t('settings.sections.network.builtInRelays.retry')}
          </Button>
        </div>
      </SettingGroup>
    )
  }

  if (!overview) {
    return (
      <SettingGroup title={title}>
        <p className="px-1 py-4 text-ui-body text-muted-foreground">
          {t('settings.sections.network.builtInRelays.loading')}
        </p>
      </SettingGroup>
    )
  }

  const builtIn = overview.entries.filter(entry => entry.source === 'builtIn')
  const dimmed = overview.savedMode !== 'builtIn'
  const routingKey = `settings.sections.network.builtInRelays.routing.${overview.savedMode}`

  return (
    <SettingGroup title={title}>
      <div className="px-1 pt-3">
        <p className="max-w-prose text-ui-caption-relaxed text-muted-foreground">
          {t('settings.sections.network.builtInRelays.description')}
        </p>
        <p
          className="mt-3 max-w-prose text-ui-body"
          data-testid="relay-overview-routing"
          data-routing-mode={overview.savedMode}
        >
          {t(routingKey)}
        </p>
        <p
          className="mt-1 max-w-prose text-ui-caption-relaxed text-muted-foreground"
          data-testid="relay-overview-applied"
          data-change-pending={overview.changePending ? 'true' : 'false'}
          data-applied-mode={overview.appliedMode ?? 'none'}
        >
          {overview.changePending
            ? t('settings.sections.network.builtInRelays.changePending')
            : overview.appliedMode
              ? t('settings.sections.network.builtInRelays.applied')
              : t('settings.sections.network.builtInRelays.notStarted')}
        </p>
        <ul
          className={cn('mt-4 flex flex-col gap-3', dimmed && 'opacity-60')}
          aria-label={title}
          data-testid="built-in-relay-list"
        >
          {builtIn.map(entry => (
            <BuiltInRelayRow key={entry.url} entry={entry} dimmed={dimmed} />
          ))}
        </ul>
      </div>
    </SettingGroup>
  )
}

function BuiltInRelayRow({ entry, dimmed }: { entry: RelayOverviewEntry; dimmed: boolean }) {
  const { t } = useTranslation()
  const labelKey = regionLabelKey(entry.regionId)
  const name = labelKey ? t(labelKey) : entry.url
  const status = entry.inEffect
    ? t('settings.sections.network.builtInRelays.status.configured')
    : dimmed
      ? t('settings.sections.network.builtInRelays.status.notUsed')
      : t('settings.sections.network.builtInRelays.status.notConfigured')

  return (
    <li
      className="flex flex-col gap-1 rounded-lg border border-border/40 px-3 py-2"
      data-testid="built-in-relay-row"
      data-region-id={entry.regionId ?? ''}
      data-in-effect={entry.inEffect ? 'true' : 'false'}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-ui-body font-medium">{name}</span>
        <span className="text-ui-caption text-muted-foreground">
          {t('settings.sections.network.builtInRelays.source')} · {status}
        </span>
      </div>
      <code className="break-all text-ui-caption text-muted-foreground">{entry.url}</code>
    </li>
  )
}
