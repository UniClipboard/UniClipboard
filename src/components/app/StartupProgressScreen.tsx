import { AlertCircle, Check, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useStartupElapsed } from '@/hooks/useStartupElapsed'
import type { StartupSnapshot } from '@/lib/startup-progress'
import { AppStateShell } from './AppStateShell'
import { resolveStartupScreenState } from './startup-screen-state'
import { StartupActions } from './StartupActions'
import { StartupActivity } from './StartupActivity'
import { StartupProgressSummary } from './StartupProgressSummary'

interface Props {
  snapshot: StartupSnapshot
  phase?: 'default' | 'membershipRecovery'
  onRetry: () => void
  onExport: () => Promise<void | boolean> | void
}

const categoryIcons = {
  failed: <AlertCircle className="size-4" />,
  ready: <Check className="size-4" />,
  working: <Loader2 className="size-4 animate-spin motion-reduce:animate-none" />,
}

export function StartupProgressScreen({ snapshot, phase = 'default', onRetry, onExport }: Props) {
  const { t } = useTranslation()
  const screen = resolveStartupScreenState(snapshot, phase)
  const elapsed = useStartupElapsed(
    snapshot.attempt_id,
    snapshot.elapsed_ms,
    !screen.failed && !screen.ready
  )

  return (
    <AppStateShell
      category={t(`upgradeProgress.${screen.category}`)}
      categoryIcon={categoryIcons[screen.icon]}
      categoryTone={screen.categoryTone}
      title={t(`upgradeProgress.${screen.title}`)}
      description={t(`upgradeProgress.${screen.description}`)}
    >
      {screen.showProgress && (
        <StartupProgressSummary
          current={screen.current}
          elapsed={elapsed}
          finishingStep={screen.finishingStep}
          percentage={screen.percentage}
        />
      )}

      {screen.showElapsed && (
        <p className="mt-5 text-ui-caption tabular-nums text-muted-foreground">
          {t('upgradeProgress.elapsed', {
            time: `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, '0')}`,
          })}
        </p>
      )}
      {screen.showActivity && <StartupActivity failed={screen.failed} snapshot={snapshot} />}
      <StartupActions
        failed={screen.failed}
        onExport={onExport}
        onRetry={onRetry}
        required={screen.required}
        snapshot={snapshot}
      />
    </AppStateShell>
  )
}
