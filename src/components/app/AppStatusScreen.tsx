import { checkForUpdate, openUpdaterWindow } from '@/api/updater'
import { Button } from '@/components/ui/button'
import type { DaemonBootstrapFailure } from '@/lib/ipc'

type AppStatusScreenProps = {
  detail: string | undefined | null
  failure?: DaemonBootstrapFailure | null
  onRetry: () => void
}

export function AppStatusScreen({ detail, failure, onRetry }: AppStatusScreenProps) {
  const versionTooOld = failure?.kind === 'versionTooOld'
  const message = versionTooOld
    ? 'A newer version is already running in the background. Please update this app to continue.'
    : "Couldn't reach the background service. Please restart the app."

  return (
    <div className="flex min-h-0 flex-1 items-center justify-center p-4 text-sm text-foreground">
      <div className="max-w-sm space-y-3 text-center">
        <p>{message}</p>
        <p className="break-words text-xs text-muted-foreground">{detail}</p>
        {versionTooOld ? (
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              void checkForUpdate(null).catch(error =>
                console.error('Update check from bootstrap error screen failed:', error)
              )
              void openUpdaterWindow().catch(error =>
                console.error('Failed to open updater window:', error)
              )
            }}
          >
            Open updater
          </Button>
        ) : (
          <Button size="sm" variant="outline" onClick={onRetry}>
            Retry
          </Button>
        )}
      </div>
    </div>
  )
}
