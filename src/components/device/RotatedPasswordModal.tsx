/**
 * RotatedPasswordModal —— 密码轮换成功后的一次性凭据展示。
 *
 * 与 MobileSyncCredentialModal 的关键差异:
 * - 没有 QR / install URL / platform tab —— 用户已经装过移动端客户端,
 *   只需把新密码填进客户端的 password 字段
 * - 没有 password 显示/隐藏切换:这是新生成的明文,显示出来就是重点
 * - 强警告:旧密码已立即失效,移动端必须更新才能继续同步
 * - 与 register 共享的不变量:password 是**唯一一次**回显,UI 必须强制
 *   用户勾选「我已保存」才允许关闭,关闭后服务端只剩 PHC,无法再取回
 */

import { Check, Copy } from 'lucide-react'
import React, { useCallback, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { type RotateMobilePasswordResult } from '@/api/tauri-command/mobile_sync'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
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
import { cn } from '@/lib/utils'

interface Props {
  /** rotate 成功的 payload。`null` 表示 modal 关闭。 */
  payload: RotateMobilePasswordResult | null
  onClose: () => void
}

const RotatedPasswordModal: React.FC<Props> = ({ payload, onClose }) => {
  const { t } = useTranslation()
  const [acknowledged, setAcknowledged] = useState(false)
  const [hintActive, setHintActive] = useState(false)
  const acknowledgeRef = useRef<HTMLLabelElement>(null)

  const handleClose = useCallback(() => {
    setAcknowledged(false)
    setHintActive(false)
    onClose()
  }, [onClose])

  const handleAcknowledge = useCallback((v: boolean) => {
    setAcknowledged(v)
    if (v) setHintActive(false)
  }, [])

  const flagUnacknowledged = useCallback(() => {
    setHintActive(true)
    acknowledgeRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }, [])

  const tryClose = useCallback(() => {
    if (!acknowledged) {
      flagUnacknowledged()
      return
    }
    handleClose()
  }, [acknowledged, flagUnacknowledged, handleClose])

  if (!payload) return null

  return (
    <Dialog
      open
      onOpenChange={open => {
        if (!open) tryClose()
      }}
    >
      <DialogContent
        className="flex max-h-[85vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-md"
        onEscapeKeyDown={e => {
          if (!acknowledged) {
            e.preventDefault()
            flagUnacknowledged()
          }
        }}
        onPointerDownOutside={e => {
          if (!acknowledged) {
            e.preventDefault()
            flagUnacknowledged()
          }
        }}
        onInteractOutside={e => {
          if (!acknowledged) {
            e.preventDefault()
            flagUnacknowledged()
          }
        }}
      >
        <DialogHeader className="px-4 pt-4 pb-2">
          <DialogTitle>{t('devices.mobileSync.rotate.result.title')}</DialogTitle>
          <DialogDescription>{t('devices.mobileSync.rotate.result.subtitle')}</DialogDescription>
        </DialogHeader>

        <div className="flex-1 space-y-4 overflow-y-auto px-4 py-2">
          {/* 强警告:旧密码立即失效 */}
          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3">
            <p className="text-xs text-amber-700/90 dark:text-amber-400/90">
              {t('devices.mobileSync.rotate.result.warning')}
            </p>
          </div>

          {/* Username (read-only, 用作"对应哪台设备" 的提示) */}
          <ReadOnlyField
            label={t('devices.mobileSync.rotate.result.usernameLabel')}
            value={payload.username}
          />

          {/* 新密码 —— 默认显示,必须显眼 */}
          <ReadOnlyField
            label={t('devices.mobileSync.rotate.result.passwordLabel')}
            value={payload.password}
            primary
          />

          {/* 强制勾选 —— 同 credential modal 的视觉模式 */}
          <label
            ref={acknowledgeRef}
            className={cn(
              'flex items-start gap-2 rounded-md border bg-card p-3 transition-all',
              hintActive
                ? 'border-primary bg-primary/10 ring-4 ring-primary/30 shadow-md shadow-primary/20'
                : 'border-border/60'
            )}
          >
            <Checkbox
              checked={acknowledged}
              onCheckedChange={v => handleAcknowledge(v === true)}
              className={cn('mt-0.5', hintActive && 'border-primary ring-3 ring-primary/40')}
            />
            <span className={cn('text-sm', hintActive && 'font-medium text-primary')}>
              {t('devices.mobileSync.rotate.result.confirmSaved')}
            </span>
          </label>
        </div>

        {hintActive && (
          <div
            className="border-t bg-destructive/5 px-4 py-2 text-xs text-destructive"
            role="alert"
          >
            {t('devices.mobileSync.rotate.result.closeBlocked')}
          </div>
        )}

        <DialogFooter className="m-0">
          <Button onClick={tryClose}>{t('devices.mobileSync.rotate.result.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

interface ReadOnlyFieldProps {
  label: string
  value: string
  /** primary=true 时给一点强调底色,用于"重点字段"如新密码。 */
  primary?: boolean
}

const ReadOnlyField: React.FC<ReadOnlyFieldProps> = ({ label, value, primary }) => {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error('Copy failed')
    }
  }, [value])

  return (
    <div className="space-y-1">
      <Label className="text-xs uppercase tracking-wider text-muted-foreground">{label}</Label>
      <div
        className={cn(
          'flex items-center gap-1 rounded-md border px-3 py-2',
          primary ? 'border-primary/40 bg-primary/5' : 'border-border/60 bg-card'
        )}
      >
        <span className="min-w-0 flex-1 truncate font-mono text-sm">{value}</span>
        <Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label={
            copied
              ? t('devices.mobileSync.rotate.result.copied')
              : t('devices.mobileSync.rotate.result.copy')
          }
          title={
            copied
              ? t('devices.mobileSync.rotate.result.copied')
              : t('devices.mobileSync.rotate.result.copy')
          }
          onClick={handleCopy}
        >
          {copied ? (
            <Check className="h-3.5 w-3.5 text-emerald-500" />
          ) : (
            <Copy className="h-3.5 w-3.5" />
          )}
        </Button>
      </div>
    </div>
  )
}

export default RotatedPasswordModal
