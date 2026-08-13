import { ShieldAlert } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DeviceTrustChoice, DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { getPendingDecisionView } from './device-trust-model'

export function DeviceTrustModal({
  snapshot,
  busy,
  error,
  onDecide,
}: {
  snapshot: DeviceTrustSnapshot
  busy: boolean
  error: string | null
  onDecide: (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => void
}) {
  const { t } = useTranslation()
  const view = useMemo(() => getPendingDecisionView(snapshot), [snapshot])
  const [confirmingKeep, setConfirmingKeep] = useState(false)
  const [confirmingExit, setConfirmingExit] = useState(false)
  const [removeTargets, setRemoveTargets] = useState(true)
  if (!view || !snapshot.currentChange) return null

  const names = (items: typeof view.targets) =>
    items.map(item => item.label).join(t('deviceTrust.listSeparator'))

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/25 p-4 backdrop-blur-xs"
      role="presentation"
    >
      <section
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="device-trust-title"
        aria-describedby="device-trust-description"
        className="max-h-[calc(100vh-2rem)] w-full max-w-xl overflow-y-auto rounded-lg border border-border bg-card text-card-foreground shadow-2xl"
      >
        <header className="flex items-start gap-3 border-b border-border px-5 py-4">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-md bg-destructive/10 text-destructive">
            <ShieldAlert className="size-5" aria-hidden="true" />
          </span>
          <div className="min-w-0">
            <h2 id="device-trust-title" className="text-base font-semibold">
              {t('deviceTrust.modal.title')}
            </h2>
            <p id="device-trust-description" className="mt-1 text-sm text-muted-foreground">
              {t('deviceTrust.modal.summary', {
                proposer: view.proposer.label,
                targets: names(view.targets),
              })}
            </p>
          </div>
        </header>

        <div className="space-y-4 px-5 py-4">
          <div className="grid gap-3 sm:grid-cols-2">
            <Impact
              title={t('deviceTrust.modal.applyTitle')}
              body={t('deviceTrust.modal.applyBody', {
                usable: names(view.apply.usable),
                removed: names(view.targets),
              })}
            />
            <Impact
              tone="danger"
              title={t('deviceTrust.modal.keepTitle')}
              body={t('deviceTrust.modal.keepBody', {
                usable: names(view.keepCurrent.usable),
                paused: names(view.keepCurrent.paused),
              })}
            />
          </div>
          {error && (
            <p className="rounded-md border border-destructive/20 bg-destructive/10 px-3 py-2 text-sm font-medium text-destructive">
              {error === 'device_state_changed'
                ? t('deviceTrust.modal.stateChanged')
                : t('deviceTrust.modal.failed')}
            </p>
          )}
          {confirmingKeep && (
            <div className="rounded-md border border-destructive/20 bg-destructive/5 p-3 text-sm">
              <p className="font-medium">{t('deviceTrust.modal.keepConfirmTitle')}</p>
              <p className="mt-1 text-muted-foreground">{t('deviceTrust.modal.keepConfirmBody')}</p>
            </div>
          )}
          {confirmingExit && (
            <div className="rounded-md border border-destructive/20 bg-destructive/5 p-3 text-sm">
              <p className="font-medium">{t('deviceTrust.modal.exitConfirmTitle')}</p>
              <p className="mt-1 text-muted-foreground">{t('deviceTrust.modal.exitConfirmBody')}</p>
            </div>
          )}
          {!view.includesLocalDevice && !confirmingKeep && !confirmingExit && (
            <label className="flex items-start gap-2 text-sm">
              <Checkbox
                checked={removeTargets}
                onCheckedChange={checked => setRemoveTargets(checked === true)}
              />
              <span>{t('deviceTrust.modal.removeTargets', { targets: names(view.targets) })}</span>
            </label>
          )}
        </div>

        <footer className="flex flex-col-reverse gap-2 border-t border-border bg-muted/40 px-5 py-4 sm:flex-row sm:justify-end">
          {confirmingKeep ? (
            <>
              <Button variant="outline" disabled={busy} onClick={() => setConfirmingKeep(false)}>
                {t('deviceTrust.actions.back')}
              </Button>
              <Button
                variant="destructive"
                disabled={busy}
                onClick={() => onDecide('keep_current_device_group', false)}
              >
                {t('deviceTrust.actions.confirmKeep')}
              </Button>
            </>
          ) : confirmingExit ? (
            <>
              <Button variant="outline" disabled={busy} onClick={() => setConfirmingExit(false)}>
                {t('deviceTrust.actions.back')}
              </Button>
              <Button
                variant="destructive"
                disabled={busy}
                onClick={() => onDecide('apply_change', true)}
              >
                {t('deviceTrust.actions.confirmExit')}
              </Button>
            </>
          ) : (
            <>
              <Button variant="outline" disabled={busy} onClick={() => setConfirmingKeep(true)}>
                {t('deviceTrust.actions.keep')}
              </Button>
              <Button
                disabled={busy}
                onClick={() => {
                  if (view.includesLocalDevice) setConfirmingExit(true)
                  else if (removeTargets) onDecide('apply_change', false)
                  else setConfirmingKeep(true)
                }}
              >
                {view.includesLocalDevice
                  ? t('deviceTrust.actions.exit')
                  : removeTargets
                    ? t('deviceTrust.actions.apply')
                    : t('deviceTrust.actions.keep')}
              </Button>
            </>
          )}
        </footer>
      </section>
    </div>
  )
}

function Impact({
  title,
  body,
  tone = 'normal',
}: {
  title: string
  body: string
  tone?: 'normal' | 'danger'
}) {
  return (
    <div
      className={
        tone === 'danger'
          ? 'rounded-md border border-destructive/20 p-3'
          : 'rounded-md border border-border p-3'
      }
    >
      <h3 className="text-sm font-semibold">{title}</h3>
      <p className="mt-1 text-xs leading-5 text-muted-foreground">{body}</p>
    </div>
  )
}
