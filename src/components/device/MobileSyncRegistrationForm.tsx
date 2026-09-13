/** Register a mobile connection and pass its one-time credentials to the device panel. */

import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import React, { useCallback, useId, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  isMobileSyncError,
  registerMobileDevice,
  type MobileSyncError,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { Input } from '@/components/ui'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Label } from '@/components/ui/label'
import { createLogger } from '@/lib/logger'

const log = createLogger('add-mobile-sync-device-dialog')

interface Props {
  footerContainer: HTMLDivElement | null
  onCancel: () => void
  onBusyChange: (busy: boolean) => void
  onSuccess: (result: RegisterMobileDeviceResult) => void
}

type FieldErrorKey = 'label' | 'username' | 'password'
type FieldErrors = Partial<Record<FieldErrorKey, string>>

const MobileSyncRegistrationForm: React.FC<Props> = ({
  onCancel,
  onBusyChange,
  onSuccess,
  footerContainer,
}) => {
  const { t } = useTranslation()
  const formId = useId()

  const [label, setLabel] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({})
  const [formError, setFormError] = useState<string | null>(null)

  const clearFieldError = useCallback((key: FieldErrorKey) => {
    setFieldErrors(prev => {
      if (prev[key] === undefined) return prev
      const next = { ...prev }
      delete next[key]
      return next
    })
  }, [])

  const handleSubmit = useCallback(async () => {
    const trimmedLabel = label.trim()
    if (trimmedLabel === '') {
      setFieldErrors({ label: t('devices.mobileSync.errors.labelEmpty') })
      setFormError(null)
      return
    }
    setSubmitting(true)
    onBusyChange(true)
    setFieldErrors({})
    setFormError(null)
    try {
      const result = await registerMobileDevice({
        label: trimmedLabel,
        username: username.trim() || undefined,
        password: password || undefined,
      })
      onSuccess(result)
    } catch (err) {
      log.error({ err }, 'failed to register mobile device')
      const dispatch = classifyRegisterError(t, err)
      if (dispatch.kind === 'field') {
        setFieldErrors({ [dispatch.field]: dispatch.message })
        // Reveal advanced fields when they contain validation errors.
        if (dispatch.field !== 'label') setAdvancedOpen(true)
      } else {
        setFormError(dispatch.message)
      }
    } finally {
      setSubmitting(false)
      onBusyChange(false)
    }
  }, [label, onBusyChange, onSuccess, password, t, username])

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
              setLabel(e.target.value)
              clearFieldError('label')
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
        <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
          <CollapsibleTrigger
            render={
              <button
                type="button"
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
                  setUsername(e.target.value)
                  clearFieldError('username')
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
                  setPassword(e.target.value)
                  clearFieldError('password')
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

type RegisterErrorDispatch =
  | { kind: 'field'; field: FieldErrorKey; message: string }
  | { kind: 'form'; message: string }

// 把后端 typed error 分流到具体字段或 form-level banner。映射规则:
// LABEL_*  → label;USERNAME_*  → username;PASSWORD_TOO_*  → password。
// 其它(facade / LAN / persistence / hash / settings / unknown)属于系统
// 级故障,不绑定到字段,统一在底部 banner 展示。
function classifyRegisterError(
  t: ReturnType<typeof useTranslation>['t'],
  err: unknown
): RegisterErrorDispatch {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'LABEL_EMPTY':
        return { kind: 'field', field: 'label', message: t('devices.mobileSync.errors.labelEmpty') }
      case 'LABEL_TOO_LONG':
        return {
          kind: 'field',
          field: 'label',
          message: t('devices.mobileSync.errors.labelTooLong', { max: e.max }),
        }
      case 'USERNAME_TAKEN':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameTaken', { username: e.username }),
        }
      case 'USERNAME_TOO_SHORT':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameTooShort', { min: e.min, got: e.got }),
        }
      case 'USERNAME_TOO_LONG':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameTooLong', { max: e.max, got: e.got }),
        }
      case 'USERNAME_MUST_START_WITH_LETTER':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameMustStartWithLetter'),
        }
      case 'USERNAME_CONTAINS_FORBIDDEN_CHARS':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameContainsForbiddenChars'),
        }
      case 'PASSWORD_TOO_SHORT':
        return {
          kind: 'field',
          field: 'password',
          message: t('devices.mobileSync.errors.passwordTooShort', { min: e.min }),
        }
      case 'PASSWORD_TOO_LONG':
        return {
          kind: 'field',
          field: 'password',
          message: t('devices.mobileSync.errors.passwordTooLong', { max: e.max }),
        }
      case 'LAN_LISTENER_DISABLED':
        return { kind: 'form', message: t('devices.mobileSync.errors.lanListenerDisabled') }
      case 'PASSWORD_HASH_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.passwordHashFailed', { message: e.message }),
        }
      case 'PERSISTENCE_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.persistenceFailed', { message: e.message }),
        }
      case 'QR_RENDER_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.qrRenderFailed', { message: e.message }),
        }
      case 'SETTINGS_LOAD_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.settingsLoadFailed', { message: e.message }),
        }
      case 'FACADE_UNAVAILABLE':
        return { kind: 'form', message: t('devices.mobileSync.errors.facadeUnavailable') }
      case 'NO_LAN_INTERFACE_AVAILABLE':
        return { kind: 'form', message: t('devices.mobileSync.errors.noLanInterfaceAvailable') }
      case 'LAN_PROBE_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.lanProbeFailed', { message: e.message }),
        }
      default: {
        // Keep unexpected service failures visible in the form.
        const message = (e as { message?: string }).message ?? e.code
        return { kind: 'form', message: t('devices.mobileSync.errors.unknown', { message }) }
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return { kind: 'form', message: t('devices.mobileSync.errors.unknown', { message }) }
}

export default MobileSyncRegistrationForm
