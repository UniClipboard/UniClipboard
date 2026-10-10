import { AlertCircle, ChevronDown } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { DaemonBootstrapFailure } from '@/lib/ipc'
import { AppStateShell } from './AppStateShell'
import { AppStatusActions } from './AppStatusActions'

type AppStatusScreenProps = {
  detail: string | undefined | null
  failure?: DaemonBootstrapFailure | null
  onRetry: () => void
  retrying?: boolean
}

export function AppStatusScreen({
  detail,
  failure,
  onRetry,
  retrying = false,
}: AppStatusScreenProps) {
  const { t } = useTranslation()
  const versionTooOld = failure?.kind === 'versionTooOld'

  return (
    <AppStateShell
      category={t('startupFailure.unavailable')}
      categoryIcon={<AlertCircle className="size-4" aria-hidden="true" />}
      categoryTone="destructive"
      title={t(versionTooOld ? 'startupFailure.updateTitle' : 'startupFailure.title')}
      description={t(
        versionTooOld ? 'startupFailure.updateDescription' : 'startupFailure.description'
      )}
    >
      <AppStatusActions onRetry={onRetry} retrying={retrying} versionTooOld={versionTooOld} />
      {detail && (
        <details className="group mt-8 border-t border-border pt-3 text-ui-body">
          <summary className="flex cursor-pointer list-none items-center justify-between text-muted-foreground hover:text-foreground [&::-webkit-details-marker]:hidden">
            {t('startupFailure.details')}
            <ChevronDown
              className="size-4 transition-transform group-open:rotate-180 motion-reduce:transition-none"
              aria-hidden="true"
            />
          </summary>
          <pre className="mt-3 max-h-40 select-text overflow-y-auto whitespace-pre-wrap break-words rounded-lg bg-muted px-3 py-2.5 font-mono text-ui-caption text-muted-foreground [overflow-wrap:anywhere]">
            {detail}
          </pre>
        </details>
      )}
    </AppStateShell>
  )
}
