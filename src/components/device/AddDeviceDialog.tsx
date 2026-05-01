import {
  AlertCircle,
  Check,
  CheckCircle2,
  Clock,
  Copy,
  Info,
  Loader2,
  RefreshCw,
  XCircle,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  cancelInvitation,
  getSetupState,
  issuePairingInvitation,
  type CurrentInvitation,
} from '@/api/daemon/setupV2'
import { onSetupInvitationRevoked, onSetupPairingCompleted } from '@/api/setupEvents'
import { formatInvitationCode } from '@/components/InvitationCodeInput'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Progress } from '@/components/ui/progress'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { useAppDispatch } from '@/store/hooks'
import { fetchSpaceMembers } from '@/store/slices/devicesSlice'

const log = createLogger('add-device-dialog')

// 默认邀请有效期 — 用于估算进度条百分比；倒计时仍按 expiresAtMs 显示真实剩余。
const DEFAULT_TTL_MS = 5 * 60 * 1000
// 配对成功后短暂展示成功态，再自动关闭对话框
const SUCCESS_AUTO_CLOSE_MS = 2000

type Step = 'invitation' | 'success' | 'failed'

interface AddDeviceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
}

export default function AddDeviceDialog({ open, onOpenChange }: AddDeviceDialogProps) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const [invitation, setInvitation] = useState<CurrentInvitation | null>(null)
  const [issuedAtMs, setIssuedAtMs] = useState<number | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [now, setNow] = useState(() => Date.now())
  const [copied, setCopied] = useState(false)
  const [step, setStep] = useState<Step>('invitation')
  const [failureReason, setFailureReason] = useState<string | null>(null)

  // ref 镜像 step / loading，供 ws 回调判断且避免 effect 因依赖变更重订阅
  const stepRef = useRef<Step>('invitation')
  const loadingRef = useRef(false)
  // 标记本次 open 是否已经初始化过邀请。effect 因 t 引用变化、Strict Mode
  // 双跑或父组件结构切换重挂载等原因被重跑时，不要重复 issue 新邀请。
  const initializedRef = useRef(false)
  useEffect(() => {
    stepRef.current = step
  }, [step])
  useEffect(() => {
    loadingRef.current = loading
  }, [loading])

  // 倒计时 tick — 仅在邀请态 + 有邀请时启动
  useEffect(() => {
    if (!open || !invitation || step !== 'invitation') return
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [open, invitation, step])

  // 打开时：优先恢复 currentInvitation，否则申请新邀请
  // 后端约束"同一时刻一个邀请"，关闭对话框不取消邀请，重开会拿回同一个
  useEffect(() => {
    if (!open) return
    // 防止 effect 重跑导致重复申请邀请。例如：配对成功后父组件因 spaceMembers
    // 变化重渲染，又或者 i18n 资源 reload 让 t 引用变化 — 这些都不应该触发
    // 第二次 issuePairingInvitation()，否则 step='success' 还没显示完就被新
    // 邀请码覆盖了。重置在 line 184 的关闭副作用里完成。
    if (initializedRef.current) return
    initializedRef.current = true
    let cancelled = false
    void (async () => {
      setLoading(true)
      setError(null)
      try {
        const state = await getSetupState()
        if (cancelled) return
        if (state.currentInvitation) {
          setInvitation(state.currentInvitation)
          // 复用的邀请 — 没有真实"签发时间"，按 TTL 倒推一个估算值
          setIssuedAtMs(state.currentInvitation.expiresAtMs - DEFAULT_TTL_MS)
        } else {
          const issued = await issuePairingInvitation()
          if (cancelled) return
          setInvitation(issued)
          setIssuedAtMs(Date.now())
        }
      } catch (err) {
        if (cancelled) return
        log.error({ err }, 'Failed to load or issue invitation')
        setError(t('devices.addDevice.errors.issueFailed'))
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [open, t])

  // 订阅 setup.pairingCompleted / setup.invitationRevoked — 后端在配对成功 /
  // 失败 / 邀请被撤销时会推送，对话框据此切换状态机
  useEffect(() => {
    if (!open) return
    let mounted = true
    const unsubs: Array<() => void> = []

    void onSetupPairingCompleted(evt => {
      if (!mounted) return
      if (stepRef.current !== 'invitation') return
      if (evt.success) {
        setStep('success')
        dispatch(fetchSpaceMembers())
      } else {
        setFailureReason(evt.reason)
        setStep('failed')
      }
    }).then(
      fn => {
        if (mounted) unsubs.push(fn)
        else fn()
      },
      err => log.warn({ err }, 'subscribe pairingCompleted failed')
    )

    void onSetupInvitationRevoked(evt => {
      if (!mounted) return
      if (stepRef.current !== 'invitation') return
      // 重新生成期间会先 cancelInvitation 触发 revoked，loading 中跳过
      if (loadingRef.current) return
      setFailureReason(evt.reason)
      setStep('failed')
    }).then(
      fn => {
        if (mounted) unsubs.push(fn)
        else fn()
      },
      err => log.warn({ err }, 'subscribe invitationRevoked failed')
    )

    return () => {
      mounted = false
      unsubs.forEach(fn => fn())
    }
  }, [open, dispatch])

  // 成功态自动关闭
  useEffect(() => {
    if (step !== 'success') return
    const id = setTimeout(() => onOpenChange(false), SUCCESS_AUTO_CLOSE_MS)
    return () => clearTimeout(id)
  }, [step, onOpenChange])

  // 关闭时清状态
  useEffect(() => {
    if (!open) {
      setInvitation(null)
      setIssuedAtMs(null)
      setError(null)
      setCopied(false)
      setStep('invitation')
      setFailureReason(null)
      initializedRef.current = false
    }
  }, [open])

  const remaining = invitation ? Math.max(0, invitation.expiresAtMs - now) : 0
  const expired = invitation && step === 'invitation' ? remaining <= 0 : false
  const totalMs = invitation && issuedAtMs ? invitation.expiresAtMs - issuedAtMs : DEFAULT_TTL_MS
  const progress = invitation ? Math.max(0, Math.min(100, (remaining / totalMs) * 100)) : 0
  const display = useMemo(
    () => (invitation ? formatInvitationCode(invitation.code) : ''),
    [invitation]
  )

  const handleCopy = async () => {
    if (!invitation) return
    try {
      await navigator.clipboard.writeText(invitation.code)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch (err) {
      log.warn({ err }, 'clipboard.writeText failed')
    }
  }

  const handleCancel = async () => {
    setLoading(true)
    try {
      await cancelInvitation()
    } catch (err) {
      log.warn({ err }, 'cancelInvitation failed (ignored on close)')
    } finally {
      setLoading(false)
      onOpenChange(false)
    }
  }

  const handleRegenerate = async () => {
    setLoading(true)
    setError(null)
    setStep('invitation')
    setFailureReason(null)
    try {
      try {
        await cancelInvitation()
      } catch (err) {
        log.warn({ err }, 'cancelInvitation before regenerate failed')
      }
      const issued = await issuePairingInvitation()
      setInvitation(issued)
      setIssuedAtMs(Date.now())
    } catch (err) {
      log.error({ err }, 'Regenerate invitation failed')
      setError(t('devices.addDevice.errors.issueFailed'))
    } finally {
      setLoading(false)
    }
  }

  const failureMessage = useMemo(() => {
    if (!failureReason) return t('devices.addDevice.failed.unknown')
    const key = `devices.addDevice.failed.reasons.${failureReason}`
    const translated = t(key)
    if (translated !== key) return translated
    return t('devices.addDevice.failed.fallback', { reason: failureReason })
  }, [failureReason, t])

  // ── 主体内容 ─────────────────────────────────────────
  let body: React.ReactNode = null
  if (step === 'success') {
    body = (
      <div className="flex flex-col items-center gap-3 py-8">
        <div className="flex h-14 w-14 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
          <CheckCircle2 className="h-8 w-8" />
        </div>
        <div className="text-center">
          <p className="text-base font-semibold text-foreground">
            {t('devices.addDevice.success.title')}
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            {t('devices.addDevice.success.subtitle')}
          </p>
        </div>
      </div>
    )
  } else if (step === 'failed') {
    body = (
      <div className="flex flex-col items-center gap-3 py-6">
        <div className="flex h-12 w-12 items-center justify-center rounded-full bg-destructive/15 text-destructive">
          <AlertCircle className="h-7 w-7" />
        </div>
        <div className="text-center">
          <p className="text-base font-semibold text-foreground">
            {t('devices.addDevice.failed.title')}
          </p>
          <p className="mt-1 text-sm text-muted-foreground">{failureMessage}</p>
        </div>
      </div>
    )
  } else if (loading && !invitation) {
    body = (
      <div className="flex items-center justify-center gap-3 py-12 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t('devices.addDevice.loading')}
      </div>
    )
  } else if (error && !invitation) {
    body = (
      <div className="flex flex-col items-center gap-3 py-10">
        <p className="text-sm text-destructive">{error}</p>
        <Button variant="outline" size="sm" onClick={handleRegenerate} disabled={loading}>
          <RefreshCw className="mr-2 h-3.5 w-3.5" />
          {t('devices.addDevice.actions.regenerate')}
        </Button>
      </div>
    )
  } else if (invitation) {
    body = (
      <div className="space-y-4 py-2">
        {/* 邀请码主卡 — 视觉焦点 */}
        <div
          className={cn(
            'rounded-2xl border bg-gradient-to-br p-5 transition-colors',
            expired
              ? 'border-destructive/30 from-destructive/5 to-transparent'
              : 'border-primary/20 from-primary/[0.04] to-transparent'
          )}
        >
          <div
            className={cn(
              'select-all text-center font-mono font-semibold tabular-nums text-foreground',
              'text-[28px] tracking-[0.18em] sm:text-[32px] sm:tracking-[0.2em]',
              expired && 'text-muted-foreground/50 line-through decoration-1'
            )}
            aria-label={invitation.code}
          >
            {display}
          </div>

          <div className="mt-5 space-y-2">
            <Progress
              value={progress}
              className={cn('h-1', expired && '[&>[data-slot=progress-indicator]]:bg-destructive')}
            />
            <div
              className={cn(
                'flex items-center justify-center gap-1.5 text-xs tabular-nums',
                expired ? 'text-destructive' : 'text-muted-foreground'
              )}
            >
              <Clock className="h-3 w-3" />
              {expired
                ? t('devices.addDevice.expired')
                : t('devices.addDevice.expiresIn', { remaining: formatRemaining(remaining) })}
            </div>
          </div>
        </div>

        {/* 提示：还需空间口令 */}
        <div className="flex items-start gap-2.5 rounded-lg bg-muted/50 px-3.5 py-2.5 text-xs text-muted-foreground">
          <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span className="leading-relaxed">{t('devices.addDevice.passphraseHint')}</span>
        </div>
      </div>
    )
  }

  // ── Footer ───────────────────────────────────────────
  let footer: React.ReactNode = null
  if (step === 'success') {
    // 自动关闭，无按钮
    footer = null
  } else if (step === 'failed') {
    footer = (
      <>
        <Button variant="outline" onClick={() => onOpenChange(false)} disabled={loading}>
          {t('devices.addDevice.actions.close')}
        </Button>
        <Button onClick={handleRegenerate} disabled={loading}>
          {loading ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" />
          )}
          {t('devices.addDevice.actions.regenerate')}
        </Button>
      </>
    )
  } else if (expired) {
    footer = (
      <>
        <Button variant="outline" onClick={() => onOpenChange(false)} disabled={loading}>
          {t('devices.addDevice.actions.close')}
        </Button>
        <Button onClick={handleRegenerate} disabled={loading}>
          {loading ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" />
          )}
          {t('devices.addDevice.actions.regenerate')}
        </Button>
      </>
    )
  } else {
    footer = (
      <>
        <Button variant="ghost" onClick={handleCancel} disabled={loading || !invitation}>
          {loading ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <XCircle className="mr-2 h-4 w-4" />
          )}
          {t('devices.addDevice.actions.cancel')}
        </Button>
        <Button
          variant={copied ? 'outline' : 'default'}
          onClick={handleCopy}
          disabled={!invitation || loading}
        >
          {copied ? (
            <>
              <Check className="mr-2 h-4 w-4" />
              {t('devices.addDevice.actions.copied')}
            </>
          ) : (
            <>
              <Copy className="mr-2 h-4 w-4" />
              {t('devices.addDevice.actions.copy')}
            </>
          )}
        </Button>
      </>
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" onInteractOutside={e => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle>
            {step === 'success'
              ? t('devices.addDevice.success.title')
              : step === 'failed'
                ? t('devices.addDevice.failed.title')
                : t('devices.addDevice.title')}
          </DialogTitle>
          {step === 'invitation' && (
            <DialogDescription>{t('devices.addDevice.subtitle')}</DialogDescription>
          )}
        </DialogHeader>

        {body}

        {footer && <DialogFooter>{footer}</DialogFooter>}
      </DialogContent>
    </Dialog>
  )
}
