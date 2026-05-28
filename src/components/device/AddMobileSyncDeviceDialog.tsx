/**
 * AddMobileSyncDeviceDialog —— 添加移动设备表单。
 *
 * 形态:label 必填 + 可选高级选项(自定义 username/password)。提交成功后
 * 关闭本 dialog,把 RegisterMobileDeviceResult 透传给上层(典型走向是
 * MobileSyncDevicesPanel 接住后立即弹 MobileSyncCredentialModal 展示一次
 * 性凭据,凭据 modal 内按平台 tab 展示具体接入步骤)。
 *
 * 凭据本身是 SyncClipboard 协议级别的(base URL + Basic Auth),与客户端平台
 * 无关 —— 注册时不要求用户选 iOS / Android,统一在 credential modal 里展示
 * 各自的接入方式。
 *
 * 错误展示:字段级错误(label / username / password 校验)就地展示在对应
 * input 下方,系统级错误(FACADE_UNAVAILABLE / LAN_* / PERSISTENCE 等)
 * 走 dialog 内底部 banner。不使用 toast —— dialog 是 portal 出去的高
 * z-index 层,toast 会被遮挡。
 */

import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import React, { useCallback, useState } from 'react'
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { createLogger } from '@/lib/logger'

const log = createLogger('add-mobile-sync-device-dialog')

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 注册成功后回调,父组件据此弹凭据 modal + 刷新列表。 */
  onSuccess: (result: RegisterMobileDeviceResult) => void
}

type FieldErrorKey = 'label' | 'username' | 'password'
type FieldErrors = Partial<Record<FieldErrorKey, string>>

const AddMobileSyncDeviceDialog: React.FC<Props> = props => {
  // 用 `open` 作 React `key`,关→开 时整个内部组件重挂载,自然带回默认
  // state(尤其密码不留)。这里替换原来的 reset-all-state on open useEffect
  // (踩 no-reset-all-state-on-prop-change)。
  return <AddMobileSyncDeviceDialogInner key={props.open ? 'open' : 'closed'} {...props} />
}

const AddMobileSyncDeviceDialogInner: React.FC<Props> = ({ open, onOpenChange, onSuccess }) => {
  const { t } = useTranslation()

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
    setFieldErrors({})
    setFormError(null)
    try {
      const result = await registerMobileDevice({
        label: trimmedLabel,
        username: username.trim() || undefined,
        password: password || undefined,
      })
      // 成功:关 dialog 让父组件弹 credential modal。注意先 onSuccess 再
      // onOpenChange(false),避免父组件还没拿到 result 就被卸载。
      onSuccess(result)
      onOpenChange(false)
    } catch (err) {
      log.error({ err }, 'failed to register mobile device')
      const dispatch = classifyRegisterError(t, err)
      if (dispatch.kind === 'field') {
        setFieldErrors({ [dispatch.field]: dispatch.message })
        // 字段错误属于高级选项时自动展开,否则用户看不到提示
        if (dispatch.field !== 'label') setAdvancedOpen(true)
      } else {
        setFormError(dispatch.message)
      }
    } finally {
      setSubmitting(false)
    }
  }, [label, onOpenChange, onSuccess, password, t, username])

  return (
    <Dialog
      open={open}
      onOpenChange={next => {
        if (!submitting) onOpenChange(next)
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('devices.mobileSync.add.title')}</DialogTitle>
          <DialogDescription>{t('devices.mobileSync.add.subtitle')}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {/* Label */}
          <div className="space-y-1.5">
            <Label htmlFor="mobile-sync-label">
              {t('devices.mobileSync.add.labelField.label')}
            </Label>
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
              <p id="mobile-sync-label-error" role="alert" className="text-xs text-destructive">
                {fieldErrors.label}
              </p>
            )}
          </div>

          {/* Advanced options */}
          <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground"
              >
                {advancedOpen ? (
                  <ChevronDown className="size-3.5" />
                ) : (
                  <ChevronRight className="size-3.5" />
                )}
                {t('devices.mobileSync.add.advanced.title')}
              </button>
            </CollapsibleTrigger>
            <CollapsibleContent className="mt-2 space-y-3 rounded-md border border-border/40 bg-muted/30 p-3">
              <p className="text-xs text-muted-foreground">
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
                    className="text-xs text-destructive"
                  >
                    {fieldErrors.username}
                  </p>
                ) : (
                  <p className="text-xs text-muted-foreground/80">
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
                    className="text-xs text-destructive"
                  >
                    {fieldErrors.password}
                  </p>
                ) : (
                  <p className="text-xs text-muted-foreground/80">
                    {t('devices.mobileSync.add.password.help')}
                  </p>
                )}
              </div>
            </CollapsibleContent>
          </Collapsible>

          {formError !== null && (
            <div
              role="alert"
              className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              {formError}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={submitting}>
            {t('devices.mobileSync.add.cancel')}
          </Button>
          <Button onClick={handleSubmit} disabled={submitting || label.trim() === ''}>
            {submitting && <Loader2 className="size-4 animate-spin" />}
            {submitting
              ? t('devices.mobileSync.add.submitting')
              : t('devices.mobileSync.add.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
        // 其余 variant 不应出现在 register 路径,落 generic 兜底
        const message = (e as { message?: string }).message ?? e.code
        return { kind: 'form', message: t('devices.mobileSync.errors.unknown', { message }) }
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return { kind: 'form', message: t('devices.mobileSync.errors.unknown', { message }) }
}

export default AddMobileSyncDeviceDialog
