/**
 * RotateMobilePasswordDialog —— 给已配对设备换密码。
 *
 * 与 AddMobileSyncDeviceDialog 的关键差异:
 * - 不要 label 输入(设备身份不变)
 * - 不要 username 自定义(server 主键稳定)
 * - 只需一个 password 输入,留空 = auto-mint
 * - 用户提交 → rotate command → 父组件接住 result,弹 RotatedPasswordModal
 *
 * 警告横幅必不可少:用户必须知道"提交后旧密码立即失效,移动端客户端
 * 在更新之前会同步失败"。这是与 register 不同的关键风险点(register 时
 * 移动端还没接入,谈不上"中断"),不能省。
 */

import { Loader2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isMobileSyncError,
  rotateMobilePassword,
  type MobileSyncError,
  type RotateMobilePasswordResult,
} from '@/api/tauri-command/mobile_sync'
import { Input } from '@/components/ui'
import { Button } from '@/components/ui/button'
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

const log = createLogger('rotate-mobile-password-dialog')

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 目标设备 —— null 时本组件不渲染。 */
  device: { deviceId: string; label: string } | null
  /** rotate 成功后回调:父组件据此弹 result modal + 刷新列表。 */
  onSuccess: (result: RotateMobilePasswordResult) => void
}

const RotateMobilePasswordDialog: React.FC<Props> = ({ open, onOpenChange, device, onSuccess }) => {
  const { t } = useTranslation()

  const [password, setPassword] = useState('')
  const [submitting, setSubmitting] = useState(false)

  // 每次重开 dialog 时重置表单 —— 避免上一次输入残留
  useEffect(() => {
    if (open) {
      setPassword('')
      setSubmitting(false)
    }
  }, [open])

  const handleSubmit = useCallback(async () => {
    if (!device) return
    setSubmitting(true)
    try {
      const result = await rotateMobilePassword({
        deviceId: device.deviceId,
        password: password.length > 0 ? password : undefined,
      })
      onSuccess(result)
      onOpenChange(false)
    } catch (err) {
      log.error({ err, deviceId: device.deviceId }, 'failed to rotate mobile password')
      toast.error(translateRotateError(t, err))
    } finally {
      setSubmitting(false)
    }
  }, [device, onOpenChange, onSuccess, password, t])

  if (!device) return null

  return (
    <Dialog
      open={open}
      onOpenChange={next => {
        if (!submitting) onOpenChange(next)
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {t('devices.mobileSync.rotate.dialog.title', { label: device.label })}
          </DialogTitle>
          <DialogDescription>{t('devices.mobileSync.rotate.dialog.subtitle')}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {/* 警告横幅 —— rotate 与 register 的关键差异 */}
          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3">
            <p className="text-xs text-amber-700/90 dark:text-amber-400/90">
              {t('devices.mobileSync.rotate.dialog.warning')}
            </p>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="rotate-password">
              {t('devices.mobileSync.rotate.dialog.passwordLabel')}
            </Label>
            <Input
              id="rotate-password"
              type="password"
              autoFocus
              value={password}
              onChange={e => setPassword(e.target.value)}
              placeholder={t('devices.mobileSync.rotate.dialog.passwordPlaceholder')}
              disabled={submitting}
              autoComplete="new-password"
            />
            <p className="text-xs text-muted-foreground/80">
              {t('devices.mobileSync.rotate.dialog.passwordHelp')}
            </p>
          </div>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={submitting}>
            {t('devices.mobileSync.rotate.dialog.cancel')}
          </Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
            {submitting
              ? t('devices.mobileSync.rotate.dialog.submitting')
              : t('devices.mobileSync.rotate.dialog.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// rotate 路径的 error 子集:与 register 共享 password 长度 / hash 失败 /
// persistence,新增 deviceNotFound(rotate 特有 — register 不会撞)。其余
// 走兜底 unknown。集中在本组件,与 panel / add dialog 的 translator 不
// 共享 —— 三个动作未来文案可能分化(rotate 强调"同步更新移动端客户端")。
function translateRotateError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'DEVICE_NOT_FOUND':
        return t('devices.mobileSync.errors.deviceNotFound')
      case 'PASSWORD_TOO_SHORT':
        return t('devices.mobileSync.errors.passwordTooShort', { min: e.min })
      case 'PASSWORD_TOO_LONG':
        return t('devices.mobileSync.errors.passwordTooLong', { max: e.max })
      case 'PASSWORD_HASH_FAILED':
        return t('devices.mobileSync.errors.passwordHashFailed', { message: e.message })
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileSync.errors.persistenceFailed', { message: e.message })
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileSync.errors.facadeUnavailable')
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileSync.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.errors.unknown', { message })
}

export const __test__ = { translateRotateError }

export default RotateMobilePasswordDialog
