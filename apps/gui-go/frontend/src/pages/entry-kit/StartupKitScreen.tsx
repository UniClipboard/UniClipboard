import type { ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'
import { STARTUP_SUPPORT_URL } from '@/api/startup-support-url'
import { resolveStartupScreenState } from '@/components/app/startup-screen-state'
import type { StartupProgressScreen as ExistingStartupProgressScreen } from '@/components/app/StartupProgressScreen'
import { useStartupActions } from '@/components/app/useStartupActions'
import { useStartupElapsed } from '@/hooks/useStartupElapsed'
import {
  Button,
  ButtonRow,
  Disclosure,
  EntryPage,
  InsetList,
  ProgressBar,
  Text,
} from '@/ui-kit/first-frame'
import appIcon from '@/updater/app-icon.png'
import { formatElapsed, statusTone } from './entry-kit-model'
import { EntryKitFrame } from './EntryKitFrame'

type Props = ComponentProps<typeof ExistingStartupProgressScreen>

/**
 * The startup, data-upgrade and space-recovery screen on the platform UI kit (design T1–T6). It
 * keeps the existing screen's props and state (`resolveStartupScreenState`, `useStartupActions`)
 * and replaces it at build time through `@ui-startup-screen`; the first frame (startup-screen.tsx)
 * and the app use this one component, so the hand-over does not change style.
 */
export function StartupProgressScreen({ snapshot, phase = 'default', onRetry, onExport }: Props) {
  const { t, i18n } = useTranslation()
  const screen = resolveStartupScreenState(snapshot, phase)
  const elapsed = useStartupElapsed(
    snapshot.attempt_id,
    snapshot.elapsed_ms,
    !screen.failed && !screen.ready
  )
  const actions = useStartupActions(onExport)
  const elapsedLabel = t('upgradeProgress.elapsed', { time: formatElapsed(elapsed) })
  const formatNumber = (value: number) => value.toLocaleString(i18n.language)
  const current = screen.current

  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{
          text: t(`upgradeProgress.${screen.category}`),
          tone: statusTone(screen.categoryTone),
          busy: screen.icon === 'working',
          icon: screen.icon === 'failed' ? 'info' : screen.icon === 'ready' ? 'shield' : undefined,
        }}
        title={t(`upgradeProgress.${screen.title}`)}
        description={t(`upgradeProgress.${screen.description}`)}
      >
        {screen.showProgress && (
          <section aria-label={t('upgradeProgress.progress')} className="flex flex-col gap-2">
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <Text emphasized>{t(`upgradeProgress.steps.${current?.step ?? 'preparing'}`)}</Text>
              <Text tone="secondary">
                {screen.percentage === null
                  ? t(
                      screen.finishingStep
                        ? 'upgradeProgress.finishingStep'
                        : 'upgradeProgress.processing'
                    )
                  : t('upgradeProgress.stepPercent', { percent: screen.percentage })}
              </Text>
            </div>
            <ProgressBar
              label={t('upgradeProgress.progress')}
              value={screen.percentage ?? undefined}
            />
            <div className="flex flex-wrap justify-between gap-2">
              <Text role="caption" tone="secondary">
                {current?.unit
                  ? t('upgradeProgress.count', {
                      processed: formatNumber(current.processed),
                      total:
                        current.total === null
                          ? t('upgradeProgress.unknown')
                          : formatNumber(current.total),
                      unit: t(`upgradeProgress.units.${current.unit}`),
                    })
                  : t('upgradeProgress.processing')}
              </Text>
              <Text role="caption" tone="secondary">
                {elapsedLabel}
              </Text>
            </div>
          </section>
        )}

        {screen.showElapsed && (
          <div className="flex flex-col gap-2">
            <ProgressBar label={t('upgradeProgress.processing')} />
            <div className="flex justify-between gap-2">
              <Text role="caption" tone="secondary">
                {t('upgradeProgress.processing')}
              </Text>
              <Text role="caption" tone="secondary">
                {elapsedLabel}
              </Text>
            </div>
          </div>
        )}

        {screen.showActivity && (
          <Disclosure label={t('upgradeProgress.activity')} defaultOpen={!screen.ready}>
            <div className="flex flex-col gap-2">
              <InsetList
                label={t('upgradeProgress.activity')}
                rows={(snapshot.upgrade?.steps ?? []).map(step => ({
                  id: step.step,
                  title: t(`upgradeProgress.steps.${step.step}`),
                  detail: t(
                    `upgradeProgress.${step.completed ? 'stepDone' : screen.failed ? 'stepStopped' : 'processing'}`
                  ),
                  tone: step.completed ? 'positive' : screen.failed ? 'critical' : 'neutral',
                }))}
              />
              {snapshot.upgrade?.steps.map(step =>
                step.warning_count === null ? (
                  <Text key={step.step} role="caption" tone="secondary">
                    {t('upgradeProgress.warningsUnknown')}
                  </Text>
                ) : step.warning_count > 0 ? (
                  <Text key={step.step} role="caption" tone="critical">
                    {t('upgradeProgress.warnings', { count: step.warning_count })}
                  </Text>
                ) : null
              )}
            </div>
          </Disclosure>
        )}

        <ButtonRow>
          {snapshot.allowed_actions.retry && screen.failed && (
            <Button variant="primary" icon="retry" onClick={onRetry}>
              {t(screen.required ? 'upgradeProgress.retry' : 'startupFailure.retry')}
            </Button>
          )}
          {screen.failed && (
            <Button
              icon="update"
              busy={actions.updateState === 'working'}
              onClick={() => void actions.checkUpdate()}
            >
              {t('settings.sections.about.checkUpdate')}
            </Button>
          )}
          {snapshot.allowed_actions.export_diagnostics && (
            <Button
              icon="download"
              busy={actions.exportState === 'working'}
              onClick={() => void actions.exportLogs()}
            >
              {t('upgradeProgress.export')}
            </Button>
          )}
          {screen.failed && (
            <Button
              variant="link"
              busy={actions.contactState === 'working'}
              onClick={() => void actions.openSupport()}
            >
              {t('startupFailure.contact')}
            </Button>
          )}
        </ButtonRow>
        {actions.exportState === 'failed' && (
          <p role="alert" className="m-0">
            <Text tone="critical">{t('upgradeProgress.exportFailed')}</Text>
          </p>
        )}
        {actions.exportState === 'done' && (
          <p role="status" className="m-0">
            <Text tone="secondary">{t('upgradeProgress.exportDone')}</Text>
          </p>
        )}
        {actions.contactState === 'failed' && (
          <p role="alert" className="m-0 flex flex-col gap-1 break-words">
            <Text tone="critical">{t('startupFailure.contactFailed')}</Text>
            <Text tone="secondary">{STARTUP_SUPPORT_URL}</Text>
          </p>
        )}
        {actions.contactState === 'done' && (
          <p role="status" className="m-0">
            <Text tone="secondary">{t('startupFailure.contactOpened')}</Text>
          </p>
        )}
        {actions.updateState === 'failed' && (
          <p role="alert" className="m-0">
            <Text tone="critical">{t('startupFailure.updateFailed')}</Text>
          </p>
        )}
      </EntryPage>
    </EntryKitFrame>
  )
}
