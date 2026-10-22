/** Register a mobile connection and pass its one-time credentials to the device panel. */

import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import React, { useId } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import { Input } from '@/components/ui'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Label } from '@/components/ui/label'
import { useMobileRegistration } from '@/hooks/useMobileRegistration'

interface Props {
  footerContainer: HTMLDivElement | null
  onCancel: () => void
  onBusyChange: (busy: boolean) => void
  onSuccess: (result: RegisterMobileDeviceResult) => void
}

const MobileSyncRegistrationForm: React.FC<Props> = ({
  onCancel,
  onBusyChange,
  onSuccess,
  footerContainer,
}) => {
  const { t } = useTranslation()
  const formId = useId()

  const {
    label,
    username,
    password,
    advancedOpen,
    submitting,
    fieldErrors,
    formError,
    dispatch,
    handleSubmit,
  } = useMobileRegistration({ onBusyChange, onSuccess })

  return (
    <form
      id={formId}
      className="flex flex-col gap-4"
      onSubmit={event => {
        event.preventDefault()
        if (!submitting) void handleSubmit()
      }}
    >
      <div className="space-y-4">
        {/* Label */}
        <div className="space-y-1.5">
          <Label htmlFor="mobile-sync-label">{t('devices.mobileSync.add.labelField.label')}</Label>
          <Input
            id="mobile-sync-label"
            autoFocus
            value={label}
            onChange={e => {
              dispatch({ type: 'edit', field: 'label', value: e.target.value })
            }}
            placeholder={t('devices.mobileSync.add.labelField.placeholder')}
            disabled={submitting}
            maxLength={64}
            aria-invalid={fieldErrors.label !== undefined || undefined}
            aria-describedby={fieldErrors.label ? 'mobile-sync-label-error' : undefined}
          />
          {fieldErrors.label !== undefined && (
            <p id="mobile-sync-label-error" role="alert" className="text-ui-body text-destructive">
              {fieldErrors.label}
            </p>
          )}
        </div>

        {/* Advanced options */}
        <Collapsible
          open={advancedOpen}
          onOpenChange={open => dispatch({ type: 'advanced', open })}
        >
          <CollapsibleTrigger
            render={
              <button
                type="button"
                aria-label={t('devices.mobileSync.add.advanced.title')}
                className="flex items-center gap-1 text-ui-body font-medium text-muted-foreground hover:text-foreground"
              />
            }
          >
            {advancedOpen ? (
              <ChevronDown className="size-3.5" />
            ) : (
              <ChevronRight className="size-3.5" />
            )}
            {t('devices.mobileSync.add.advanced.title')}
          </CollapsibleTrigger>
          <CollapsibleContent className="mt-2 space-y-3 rounded-md border border-border/40 bg-muted/30 p-3">
            <p className="text-ui-caption text-muted-foreground">
              {t('devices.mobileSync.add.advanced.description')}
            </p>

            <div className="space-y-1.5">
              <Label htmlFor="mobile-sync-username">
                {t('devices.mobileSync.add.username.label')}
              </Label>
              <Input
                id="mobile-sync-username"
                value={username}
                onChange={e => {
                  dispatch({
                    type: 'edit',
                    field: 'username',
                    value: e.target.value,
                  })
                }}
                placeholder={t('devices.mobileSync.add.username.placeholder')}
                disabled={submitting}
                autoComplete="off"
                aria-invalid={fieldErrors.username !== undefined || undefined}
                aria-describedby={fieldErrors.username ? 'mobile-sync-username-error' : undefined}
              />
              {fieldErrors.username !== undefined ? (
                <p
                  id="mobile-sync-username-error"
                  role="alert"
                  className="text-ui-body text-destructive"
                >
                  {fieldErrors.username}
                </p>
              ) : (
                <p className="text-ui-caption text-muted-foreground/80">
                  {t('devices.mobileSync.add.username.help')}
                </p>
              )}
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="mobile-sync-password">
                {t('devices.mobileSync.add.password.label')}
              </Label>
              <Input
                id="mobile-sync-password"
                type="password"
                value={password}
                onChange={e => {
                  dispatch({
                    type: 'edit',
                    field: 'password',
                    value: e.target.value,
                  })
                }}
                placeholder={t('devices.mobileSync.add.password.placeholder')}
                disabled={submitting}
                autoComplete="new-password"
                aria-invalid={fieldErrors.password !== undefined || undefined}
                aria-describedby={fieldErrors.password ? 'mobile-sync-password-error' : undefined}
              />
              {fieldErrors.password !== undefined ? (
                <p
                  id="mobile-sync-password-error"
                  role="alert"
                  className="text-ui-body text-destructive"
                >
                  {fieldErrors.password}
                </p>
              ) : (
                <p className="text-ui-caption text-muted-foreground/80">
                  {t('devices.mobileSync.add.password.help')}
                </p>
              )}
            </div>
          </CollapsibleContent>
        </Collapsible>

        {formError !== null && (
          <div
            role="alert"
            className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-ui-body text-destructive"
          >
            {formError}
          </div>
        )}
      </div>

      {footerContainer &&
        createPortal(
          <>
            <Button variant="ghost" onClick={onCancel} disabled={submitting}>
              {t('devices.mobileSync.add.cancel')}
            </Button>
            <Button type="submit" form={formId} disabled={submitting || label.trim() === ''}>
              {submitting && <Loader2 className="size-4 animate-spin" />}
              {submitting
                ? t('devices.mobileSync.add.submitting')
                : t('devices.mobileSync.add.submit')}
            </Button>
          </>,
          footerContainer
        )}
    </form>
  )
}

export default MobileSyncRegistrationForm
