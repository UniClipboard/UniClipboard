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
 */

import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
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
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('add-mobile-sync-device-dialog')

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 注册成功后回调,父组件据此弹凭据 modal + 刷新列表。 */
  onSuccess: (result: RegisterMobileDeviceResult) => void
}

const AddMobileSyncDeviceDialog: React.FC<Props> = ({ open, onOpenChange, onSuccess }) => {
  const { t } = useTranslation()

  const [label, setLabel] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [submitting, setSubmitting] = useState(false)

  // 每次重开 dialog 时重置表单 —— 避免上一次输入残留(尤其密码)
  useEffect(() => {
    if (open) {
      setLabel('')
      setUsername('')
      setPassword('')
      setAdvancedOpen(false)
      setSubmitting(false)
    }
  }, [open])

  const handleSubmit = useCallback(async () => {
    const trimmedLabel = label.trim()
    if (trimmedLabel === '') {
      toast.error(t('devices.mobileSync.errors.labelEmpty'))
      return
    }
    setSubmitting(true)
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
      toast.error(translateRegisterError(t, err))
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
              onChange={e => setLabel(e.target.value)}
              placeholder={t('devices.mobileSync.add.labelField.placeholder')}
              disabled={submitting}
              maxLength={64}
            />
          </div>

          {/* Advanced options */}
          <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground"
              >
                {advancedOpen ? (
                  <ChevronDown className="h-3.5 w-3.5" />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5" />
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
                  onChange={e => setUsername(e.target.value)}
                  placeholder={t('devices.mobileSync.add.username.placeholder')}
                  disabled={submitting}
                  autoComplete="off"
                />
                <p className="text-xs text-muted-foreground/80">
                  {t('devices.mobileSync.add.username.help')}
                </p>
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="mobile-sync-password">
                  {t('devices.mobileSync.add.password.label')}
                </Label>
                <Input
                  id="mobile-sync-password"
                  type="password"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  placeholder={t('devices.mobileSync.add.password.placeholder')}
                  disabled={submitting}
                  autoComplete="new-password"
                />
                <p className="text-xs text-muted-foreground/80">
                  {t('devices.mobileSync.add.password.help')}
                </p>
              </div>
            </CollapsibleContent>
          </Collapsible>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={submitting}>
            {t('devices.mobileSync.add.cancel')}
          </Button>
          <Button onClick={handleSubmit} disabled={submitting || label.trim() === ''}>
            {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
            {submitting
              ? t('devices.mobileSync.add.submitting')
              : t('devices.mobileSync.add.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// Register-specific error translator: 沿用 panel 的 i18n key 表,但只覆盖
// register 路径会触发的 variant + 兜底。集中在一处方便后续 add dialog 单独
// 演化(panel 也仍有自己的 translateMobileSyncError, 不共享是有意 —— 两条
// 错误路径未来文案可能分化, 例如 add 页要更详细的指引)。
function translateRegisterError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'LABEL_EMPTY':
        return t('devices.mobileSync.errors.labelEmpty')
      case 'LABEL_TOO_LONG':
        return t('devices.mobileSync.errors.labelTooLong', { max: e.max })
      case 'LAN_LISTENER_DISABLED':
        return t('devices.mobileSync.errors.lanListenerDisabled')
      case 'USERNAME_TAKEN':
        return t('devices.mobileSync.errors.usernameTaken', { username: e.username })
      case 'USERNAME_INVALID_SHAPE':
        return t('devices.mobileSync.errors.usernameInvalidShape', { reason: e.reason })
      case 'PASSWORD_TOO_SHORT':
        return t('devices.mobileSync.errors.passwordTooShort', { min: e.min })
      case 'PASSWORD_TOO_LONG':
        return t('devices.mobileSync.errors.passwordTooLong', { max: e.max })
      case 'PASSWORD_HASH_FAILED':
        return t('devices.mobileSync.errors.passwordHashFailed', { message: e.message })
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileSync.errors.persistenceFailed', { message: e.message })
      case 'QR_RENDER_FAILED':
        return t('devices.mobileSync.errors.qrRenderFailed', { message: e.message })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileSync.errors.settingsLoadFailed', { message: e.message })
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileSync.errors.facadeUnavailable')
      case 'NO_LAN_INTERFACE_AVAILABLE':
        return t('devices.mobileSync.errors.noLanInterfaceAvailable')
      case 'LAN_PROBE_FAILED':
        return t('devices.mobileSync.errors.lanProbeFailed', { message: e.message })
      default: {
        // 其余 variant 不应出现在 register 路径,落 generic 兜底
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileSync.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.errors.unknown', { message })
}

export default AddMobileSyncDeviceDialog
