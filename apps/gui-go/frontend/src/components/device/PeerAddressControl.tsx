import { useId } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { usePeerAddress } from '@/hooks/usePeerAddress'

export function PeerAddressControl({ deviceId }: { deviceId: string }) {
  const { t } = useTranslation()
  const address = usePeerAddress(deviceId)
  const id = useId()
  return (
    <section className="space-y-3 rounded-xl border border-border/60 bg-card p-5 text-card-foreground">
      <h4 className="text-ui-body font-medium">{t('devices.directAddress.title')}</h4>
      <p id={`${id}-hint`} className="text-ui-caption text-muted-foreground">
        {t('devices.directAddress.hint')}
      </p>
      <form
        onSubmit={event => {
          event.preventDefault()
          void address.save()
        }}
      >
        <fieldset disabled={address.disabled} className="space-y-3">
          <div className="flex flex-wrap gap-3">
            <div className="min-w-0 flex-1 space-y-1">
              <label htmlFor={`${id}-ip`} className="text-ui-caption">
                {t('devices.directAddress.ip')}
              </label>
              <Input
                id={`${id}-ip`}
                value={address.ip}
                autoComplete="off"
                onChange={event => address.setIp(event.target.value)}
                aria-invalid={!!address.ipError}
                aria-describedby={address.ipError ? `${id}-ip-error` : `${id}-hint`}
              />
              {address.ipError && (
                <p id={`${id}-ip-error`} className="text-ui-caption text-destructive">
                  {address.ipError}
                </p>
              )}
            </div>
            <div className="w-28 space-y-1">
              <label htmlFor={`${id}-port`} className="text-ui-caption">
                {t('devices.directAddress.port')}
              </label>
              <Input
                id={`${id}-port`}
                value={address.port}
                inputMode="numeric"
                autoComplete="off"
                onChange={event => address.setPort(event.target.value)}
                aria-invalid={!!address.portError}
                aria-describedby={address.portError ? `${id}-port-error` : undefined}
              />
              {address.portError && (
                <p id={`${id}-port-error`} className="text-ui-caption text-destructive">
                  {address.portError}
                </p>
              )}
            </div>
          </div>
          <div className="flex gap-2">
            <Button type="submit" size="sm" disabled={!address.canSave} aria-busy={address.busy}>
              {t(address.busy ? 'devices.directAddress.saving' : 'devices.directAddress.save')}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!address.canClear}
              onClick={() => {
                void address.clear()
              }}
            >
              {t('devices.directAddress.clear')}
            </Button>
          </div>
        </fieldset>
      </form>
      {address.error && (
        <p role="alert" className="text-ui-caption text-destructive">
          {address.error}
        </p>
      )}
    </section>
  )
}
