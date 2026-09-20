import { AlertCircle } from 'lucide-react'
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
        <details className="mt-8 border-t border-border pt-4 text-ui-body">
          <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
            {t('startupFailure.details')}
          </summary>
          <pre className="mt-3 max-h-40 select-text overflow-y-auto whitespace-pre-wrap break-words font-mono text-ui-caption text-muted-foreground [overflow-wrap:anywhere]">
            {detail}
          </pre>
        </details>
      )}
    </AppStateShell>
  )
}
