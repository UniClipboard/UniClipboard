import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

export function RelayCredentialStatus({ configured }: { configured: boolean | null }) {
  const { t } = useTranslation()
  return (
    <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <span
        aria-hidden="true"
        className={cn(
          'size-1.5 rounded-full',
          configured === null
            ? 'animate-pulse bg-muted-foreground/40'
            : configured
              ? 'bg-emerald-500'
              : 'bg-muted-foreground/40'
        )}
      />
      {configured === null
        ? t('settings.sections.network.customRelays.credentials.loading')
        : configured
          ? t('settings.sections.network.customRelays.credentials.configured')
          : t('settings.sections.network.customRelays.credentials.notConfigured')}
    </span>
  )
}
