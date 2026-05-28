import { AlertCircle, ArrowRightLeft, CheckCircle2, Eye, EyeOff, Loader2 } from 'lucide-react'
import { useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { Trans, useTranslation } from 'react-i18next'
import {
  queryMigrationProgress,
  SetupV2Error,
  switchSpace,
  type MigrationPhase,
  type SwitchSpaceErrorKind,
  type SwitchSpaceResponse,
} from '@/api/daemon/setupV2'
import { INVITATION_CODE_LENGTH } from '@/components/invitation-code-utils'
import { InvitationCodeInput } from '@/components/InvitationCodeInput'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { useAppDispatch } from '@/store/hooks'
import { fetchLocalDeviceInfo, fetchSpaceMembers } from '@/store/slices/devicesSlice'

const log = createLogger('switch-space-dialog')

const PROGRESS_POLL_MS = 1000
const SUCCESS_AUTO_CLOSE_MS = 2500

type Step = 'input' | 'migrating' | 'success' | 'failed'

interface SwitchSpaceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * "加入其他空间" 流程的 dialog。
 *
 * 状态机：input → migrating → (success | failed)。
 * - input：邀请码 + 新口令输入；提交后进入 migrating。
 * - migrating：发起 switchSpace HTTP 请求，并以 1s 间隔轮询
 *   queryMigrationProgress 把当前 phase 显示给用户。
 * - success：展示 migrated_records 后自动关闭，并刷新 roster。
 * - failed：按 SwitchSpaceErrorKind 显示具体提示，提供重试。
 */
export default function SwitchSpaceDialog({ open, onOpenChange }: SwitchSpaceDialogProps) {
  // `open` is used as a React `key` on the inner body. When the dialog
  // closes, the inner component unmounts and its state is dropped — the
  // next open remounts with fresh defaults. This replaces the previous
  // reset-all-state-on-open useEffect (no-reset-all-state-on-prop-change).
  return (
    <SwitchSpaceDialogInner
      key={open ? 'open' : 'closed'}
      open={open}
      onOpenChange={onOpenChange}
    />
  )
}

function SwitchSpaceDialogInner({ open, onOpenChange }: SwitchSpaceDialogProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'devices.switchSpace' })
  const dispatch = useAppDispatch()
  const [step, setStep] = useState<Step>('input')
  const [code, setCode] = useState('')
  const [pass, setPass] = useState('')
  const [showPass, setShowPass] = useState(false)
  const [errorKind, setErrorKind] = useState<SwitchSpaceErrorKind | null>(null)
  const [errorRaw, setErrorRaw] = useState<string | null>(null)
  const [phase, setPhase] = useState<MigrationPhase | null>(null)
  const [backupCount, setBackupCount] = useState(0)
  const [result, setResult] = useState<SwitchSpaceResponse | null>(null)
  const passInputRef = useRef<HTMLInputElement>(null)

  const codeComplete = code.length === INVITATION_CODE_LENGTH
  const canSubmit = codeComplete && pass.length > 0 && step === 'input'

  // 输完邀请码自动焦点跳到 passphrase（与 RedeemInvitationScreen 一致）
  useEffect(() => {
    if (codeComplete && step === 'input') passInputRef.current?.focus()
  }, [codeComplete, step])

  // 迁移进度轮询：phase 显示哪一步、backup_record_count 提示规模
  useEffect(() => {
    if (step !== 'migrating') return
    let cancelled = false
    const tick = async () => {
      try {
        const p = await queryMigrationProgress()
        if (cancelled) return
        setPhase(p.phase)
        setBackupCount(p.backupRecordCount)
      } catch (err) {
        log.warn({ err }, 'queryMigrationProgress failed (will retry)')
      }
    }
    void tick()
    const id = setInterval(() => {
      void tick()
    }, PROGRESS_POLL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [step])

  // success 自动关闭 + 刷新 devices state
  //
  // 用 useEffectEvent 把 onOpenChange 从 effect 依赖里挪出去 —— 它仅在
  // setTimeout 触发时被读取一次，没必要因为父组件重新创建函数引用而让
  // effect 重新订阅（避免重复跑 dispatch / 提前重置 timeout）。
  const closeDialog = useEffectEvent(() => onOpenChange(false))
  useEffect(() => {
    if (step !== 'success') return
    dispatch(fetchSpaceMembers())
    dispatch(fetchLocalDeviceInfo())
    const id = setTimeout(() => closeDialog(), SUCCESS_AUTO_CLOSE_MS)
    return () => clearTimeout(id)
  }, [step, dispatch])

  const handleSubmit = async () => {
    if (!canSubmit) return
    setErrorKind(null)
    setErrorRaw(null)
    setStep('migrating')
    try {
      const res = await switchSpace({ code, newPassphrase: pass })
      setResult(res)
      setStep('success')
    } catch (err) {
      log.error({ err }, 'switchSpace failed')
      if (err instanceof SetupV2Error) {
        setErrorKind(err.kind as SwitchSpaceErrorKind)
        setErrorRaw(err.raw)
      } else {
        setErrorKind('internal')
        setErrorRaw(err instanceof Error ? err.message : String(err))
      }
      setStep('failed')
    }
  }

  // 邀请码已废类（被 sponsor consume、过期、或 sponsor 不认）—— 原 code 不可能再
  // resolve 成功，"重试"按钮文案与回到 input 时的清空策略都按这个走。
  const isCodeDead =
    errorKind === 'invitation_not_found' ||
    errorKind === 'invitation_expired' ||
    errorKind === 'sponsor_rejected'

  const handleRetry = () => {
    if (isCodeDead) {
      setCode('')
      setPass('')
    } else if (errorKind === 'passphrase_mismatch') {
      setPass('')
    }
    setErrorKind(null)
    setErrorRaw(null)
    setStep('input')
  }

  const retryLabel = isCodeDead ? t('actions.useNewCode') : t('actions.retry')

  const failureMessage = useMemo(() => {
    if (!errorKind) return null
    const key = `failed.reasons.${errorKind}`
    const translated = t(key)
    if (translated !== key) return translated
    return t('failed.fallback', { reason: errorRaw ?? errorKind })
  }, [errorKind, errorRaw, t])

  const phaseLabel = useMemo(() => {
    // phase=null 代表轮询尚未拿到首个进度（switchSpace 已发出但 backend
    // 还没写 Prepared）——按"准备中"语义对待。
    if (phase === null) return t('migrating.phase.preparing')
    if (phase === 'prepared') return t('migrating.phase.prepared')
    if (phase === 'handshake_done') return t('migrating.phase.handshakeDone')
    return t('migrating.phase.swapped')
  }, [phase, t])

  // ── 主体内容 ─────────────────────────────────────────
  let body: React.ReactNode
  if (step === 'input') {
    body = (
      <div className="space-y-5 py-2">
        <div className="space-y-2">
          <Label htmlFor="switch-code" className="sr-only">
            {t('labels.code')}
          </Label>
          <InvitationCodeInput
            id="switch-code"
            value={code}
            onChange={setCode}
            invalid={errorKind === 'invitation_not_found' || errorKind === 'invitation_expired'}
            autoFocus
          />
        </div>

        {codeComplete && (
          <div className="space-y-2">
            <Label htmlFor="switch-pass" className="text-xs text-muted-foreground">
              {t('labels.newPassphrase')}
            </Label>
            <div className="relative">
              <Input
                id="switch-pass"
                ref={passInputRef}
                type={showPass ? 'text' : 'password'}
                value={pass}
                onChange={e => setPass(e.target.value)}
                placeholder={t('placeholders.newPassphrase')}
                className="h-10 pr-10"
                onKeyDown={e => {
                  if (e.key === 'Enter') void handleSubmit()
                }}
              />
              <button
                type="button"
                onClick={() => setShowPass(s => !s)}
                className="absolute right-0 top-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground"
                tabIndex={-1}
                aria-label={showPass ? 'hide passphrase' : 'show passphrase'}
              >
                {showPass ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
              </button>
            </div>
          </div>
        )}

        <div className="flex items-start gap-2.5 rounded-lg border border-amber-500/30 bg-amber-500/5 px-3.5 py-2.5 text-xs text-muted-foreground">
          <AlertCircle className="mt-0.5 size-4 shrink-0 text-amber-500" />
          <span className="leading-relaxed">{t('warning')}</span>
        </div>
      </div>
    )
  } else if (step === 'migrating') {
    body = (
      <div className="flex flex-col items-center gap-4 py-8">
        <div className="flex size-14 items-center justify-center rounded-full bg-primary/10 text-primary">
          <Loader2 className="size-7 animate-spin" />
        </div>
        <div className="text-center">
          <p className="text-base font-semibold text-foreground">{t('migrating.title')}</p>
          <p className="mt-1 text-sm text-muted-foreground">{phaseLabel}</p>
          {backupCount > 0 && (
            <p className="mt-2 text-xs text-muted-foreground">
              {t('migrating.recordsHint', { count: backupCount })}
            </p>
          )}
        </div>
      </div>
    )
  } else if (step === 'success' && result) {
    body = (
      <div className="flex flex-col items-center gap-3 py-8">
        <div className="flex size-14 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
          <CheckCircle2 className="size-8" />
        </div>
        <div className="text-center">
          <p className="text-base font-semibold text-foreground">{t('success.title')}</p>
          <p className="mt-1 text-sm text-muted-foreground">
            <Trans
              t={t}
              i18nKey="success.subtitle"
              count={result.migratedRecords}
              values={{ count: result.migratedRecords }}
            />
          </p>
        </div>
      </div>
    )
  } else {
    body = (
      <div className="flex flex-col items-center gap-3 py-6">
        <div className="flex size-12 items-center justify-center rounded-full bg-destructive/15 text-destructive">
          <AlertCircle className="size-7" />
        </div>
        <div className="text-center">
          <p className="text-base font-semibold text-foreground">{t('failed.title')}</p>
          {failureMessage && <p className="mt-1 text-sm text-muted-foreground">{failureMessage}</p>}
        </div>
      </div>
    )
  }

  // ── Footer ───────────────────────────────────────────
  let footer: React.ReactNode = null
  if (step === 'input') {
    footer = (
      <>
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          {t('actions.cancel')}
        </Button>
        <Button onClick={handleSubmit} disabled={!canSubmit} className="min-w-28">
          <ArrowRightLeft className={cn('mr-2 size-4', !canSubmit && 'opacity-50')} />
          {t('actions.switch')}
        </Button>
      </>
    )
  } else if (step === 'migrating') {
    // 迁移进行中——禁用所有交互，避免用户中断。Ctrl-C / 关闭窗口仍然可
    // 以触发，但 onInteractOutside / onEscapeKeyDown 已经在 DialogContent
    // 上拦截。
    footer = (
      <Button variant="outline" disabled>
        <Loader2 className="mr-2 size-4 animate-spin" />
        {t('actions.switching')}
      </Button>
    )
  } else if (step === 'failed') {
    footer = (
      <>
        <Button variant="outline" onClick={() => onOpenChange(false)}>
          {t('actions.close')}
        </Button>
        <Button onClick={handleRetry}>{retryLabel}</Button>
      </>
    )
  }
  // success: 自动关闭，无 footer

  return (
    <Dialog
      open={open}
      onOpenChange={next => {
        // 迁移中阻止关闭，避免用户误触导致状态机从 GUI 视角看起来"丢失"
        if (step === 'migrating' && !next) return
        onOpenChange(next)
      }}
    >
      <DialogContent
        className="sm:max-w-md"
        onInteractOutside={e => {
          if (step === 'migrating') e.preventDefault()
        }}
        onEscapeKeyDown={e => {
          if (step === 'migrating') e.preventDefault()
        }}
      >
        <DialogHeader>
          <DialogTitle>
            {step === 'success'
              ? t('success.title')
              : step === 'failed'
                ? t('failed.title')
                : step === 'migrating'
                  ? t('migrating.title')
                  : t('title')}
          </DialogTitle>
          {step === 'input' && <DialogDescription>{t('subtitle')}</DialogDescription>}
        </DialogHeader>

        {body}

        {footer && <DialogFooter>{footer}</DialogFooter>}
      </DialogContent>
    </Dialog>
  )
}
