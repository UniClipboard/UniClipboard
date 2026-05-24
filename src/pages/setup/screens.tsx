import { AnimatePresence, motion } from 'framer-motion'
import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  CheckCircle2,
  Eye,
  EyeOff,
  Loader2,
  Shield,
  Smartphone,
  XCircle,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { getSettings } from '@/api/daemon/settings'
import type {
  InitializeSpaceErrorKind,
  RedeemInvitationErrorKind,
  RedeemResponse,
} from '@/api/daemon/setupV2'
import {
  INVITATION_CODE_LENGTH,
  InvitationCodeInput,
  formatInvitationCode,
} from '@/components/InvitationCodeInput'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'

// ── Common shell ───────────────────────────────────────────────────────────

function ScreenShell({
  title,
  subtitle,
  children,
  footer,
  hint,
  error,
  centered = false,
}: {
  title: string
  subtitle?: string
  children?: ReactNode
  footer?: ReactNode
  hint?: ReactNode
  error?: string | null
  centered?: boolean
}) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -12 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="w-full"
    >
      <div className={cn('text-foreground', centered && 'text-center')}>
        <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{title}</h1>
        {subtitle && <p className="mt-2 text-muted-foreground">{subtitle}</p>}
      </div>

      {children}

      {error && (
        <div
          role="alert"
          className={cn(
            'mt-4 flex items-center gap-2 text-sm text-destructive sm:mt-5',
            centered && 'justify-center'
          )}
        >
          <AlertCircle className="h-4 w-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {footer && (
        <div className={cn('mt-7 flex sm:mt-8', centered && 'justify-center')}>{footer}</div>
      )}

      {hint && <div className="mt-4 text-xs text-muted-foreground sm:mt-5">{hint}</div>}
    </motion.div>
  )
}

// ── S0 — Entry ─────────────────────────────────────────────────────────────

export function EntryScreen({
  onCreate,
  onJoin,
  loading,
}: {
  onCreate: () => void
  onJoin: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -12 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="w-full"
    >
      <div className="mb-8 text-center sm:mb-10">
        <h1 className="text-3xl font-semibold tracking-tight text-foreground sm:text-4xl">
          {t('title')}
        </h1>
        <p className="mt-4 text-lg text-muted-foreground">{t('subtitle')}</p>
      </div>

      <div className="flex flex-row gap-4">
        <button
          type="button"
          data-testid="setup-entry-create"
          onClick={onCreate}
          disabled={loading}
          className="group relative flex flex-1 flex-col items-start gap-5 rounded-xl border border-white/20 bg-white/40 p-7 text-left backdrop-blur-xl transition-all duration-300 hover:-translate-y-1 hover:border-white/40 hover:bg-white/50 hover:shadow-lg active:translate-y-0 active:shadow-sm disabled:opacity-50 dark:border-white/10 dark:bg-white/5 dark:hover:border-white/20 dark:hover:bg-white/10"
        >
          <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-primary/10 text-primary">
            <Shield className="h-6 w-6" />
          </div>
          <div className="space-y-2">
            <h3 className="text-lg font-medium text-foreground">{t('create.title')}</h3>
            <p className="text-sm leading-relaxed text-muted-foreground">
              {t('create.description')}
            </p>
          </div>
          <div className="mt-auto flex items-center gap-2 text-sm font-medium text-primary">
            {t('create.cta')}
            <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-1" />
          </div>
        </button>

        <button
          type="button"
          data-testid="setup-entry-join"
          onClick={onJoin}
          disabled={loading}
          className="group relative flex flex-1 flex-col items-start gap-5 rounded-xl border border-white/20 bg-white/40 p-7 text-left backdrop-blur-xl transition-all duration-300 hover:-translate-y-1 hover:border-white/40 hover:bg-white/50 hover:shadow-lg active:translate-y-0 active:shadow-sm disabled:opacity-50 dark:border-white/10 dark:bg-white/5 dark:hover:border-white/20 dark:hover:bg-white/10"
        >
          <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-primary/10 text-primary">
            <Smartphone className="h-6 w-6" />
          </div>
          <div className="space-y-2">
            <h3 className="text-lg font-medium text-foreground">{t('join.title')}</h3>
            <p className="text-sm leading-relaxed text-muted-foreground">{t('join.description')}</p>
          </div>
          <div className="mt-auto flex items-center gap-2 text-sm font-medium text-primary">
            {t('join.cta')}
            <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-1" />
          </div>
        </button>
      </div>

      <div className="mt-8 text-center text-xs text-muted-foreground sm:mt-10">{t('footer')}</div>
    </motion.div>
  )
}

// ── S1 — Initialize space ───────────────────────────────────────────────────

function initializeErrorMessage(
  t: (k: string) => string,
  kind: InitializeSpaceErrorKind | null
): string | null {
  switch (kind) {
    case null:
      return null
    case 'passphrase_mismatch':
      return t('errors.passphraseMismatch')
    case 'device_name_required':
      return t('errors.deviceNameRequired')
    case 'already_initialized':
    case 'already_setup':
      return t('errors.alreadyInitialized')
    case 'service_unavailable':
      return t('errors.serviceUnavailable')
    case 'internal':
    default:
      return t('errors.generic')
  }
}

export function InitializeSpaceScreen({
  onSubmit,
  onBack,
  loading,
}: {
  onSubmit: (input: {
    deviceName: string
    passphrase: string
    passphraseConfirm: string
  }) => Promise<{ ok: true } | { ok: false; kind: InitializeSpaceErrorKind; raw: string }>
  onBack: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.initializeSpace' })
  const [deviceName, setDeviceName] = useState('')
  const [pass1, setPass1] = useState('')
  const [pass2, setPass2] = useState('')
  const [showPass1, setShowPass1] = useState(false)
  const [showPass2, setShowPass2] = useState(false)
  const [errorKind, setErrorKind] = useState<InitializeSpaceErrorKind | null>(null)

  const errorMessage = initializeErrorMessage(t, errorKind)

  // Pre-fill the device name with the daemon-resolved default (the OS
  // hostname, written during bootstrap). Skip the write if the user has
  // already typed something so a late response never clobbers their input.
  useEffect(() => {
    let cancelled = false
    getSettings()
      .then(s => {
        if (cancelled) return
        const fallback = s.general.deviceName?.trim() ?? ''
        if (!fallback) return
        setDeviceName(prev => (prev ? prev : fallback))
      })
      .catch(() => {
        // Non-fatal — user can still type a name manually.
      })
    return () => {
      cancelled = true
    }
  }, [])

  const handleSubmit = async () => {
    setErrorKind(null)
    if (!deviceName.trim()) {
      setErrorKind('device_name_required')
      return
    }
    if (!pass1) {
      setErrorKind('passphrase_mismatch')
      return
    }
    if (pass1 !== pass2) {
      setErrorKind('passphrase_mismatch')
      return
    }
    const res = await onSubmit({
      deviceName: deviceName.trim(),
      passphrase: pass1,
      passphraseConfirm: pass2,
    })
    if (!res.ok) setErrorKind(res.kind)
  }

  return (
    <ScreenShell
      title={t('title')}
      subtitle={t('subtitle')}
      error={errorMessage}
      hint={t('hint')}
      footer={
        <div className="flex w-full items-center justify-between gap-3">
          <Button
            variant="ghost"
            data-testid="setup-initialize-back"
            onClick={onBack}
            disabled={loading}
          >
            <ArrowLeft className="mr-2 h-4 w-4" />
            {t('actions.back')}
          </Button>
          <Button
            data-testid="setup-initialize-submit"
            onClick={handleSubmit}
            disabled={loading}
            className="min-w-32"
          >
            {loading ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                {t('actions.creating')}
              </>
            ) : (
              t('actions.submit')
            )}
          </Button>
        </div>
      }
    >
      <div className="mt-6 space-y-6 sm:mt-8">
        <div className="space-y-2">
          <Label htmlFor="device-name">{t('labels.deviceName')}</Label>
          <Input
            id="device-name"
            value={deviceName}
            onChange={e => setDeviceName(e.target.value)}
            disabled={loading}
            placeholder={t('placeholders.deviceName')}
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="pass1">{t('labels.passphrase')}</Label>
          <div className="relative">
            <Input
              id="pass1"
              type={showPass1 ? 'text' : 'password'}
              value={pass1}
              onChange={e => setPass1(e.target.value)}
              disabled={loading}
              className="pr-10"
              placeholder={t('placeholders.passphrase')}
            />
            <button
              type="button"
              onClick={() => setShowPass1(!showPass1)}
              className="absolute right-0 top-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground"
            >
              {showPass1 ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
            </button>
          </div>
        </div>

        <div className="space-y-2">
          <Label htmlFor="pass2">{t('labels.passphraseConfirm')}</Label>
          <div className="relative">
            <Input
              id="pass2"
              type={showPass2 ? 'text' : 'password'}
              value={pass2}
              onChange={e => setPass2(e.target.value)}
              disabled={loading}
              className="pr-10"
              placeholder={t('placeholders.passphraseConfirm')}
              onKeyDown={e => e.key === 'Enter' && handleSubmit()}
            />
            <button
              type="button"
              onClick={() => setShowPass2(!showPass2)}
              className="absolute right-0 top-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground"
            >
              {showPass2 ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
            </button>
          </div>
        </div>
      </div>
    </ScreenShell>
  )
}

// ── S3 — Show invitation ────────────────────────────────────────────────────

function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
}

export function ShowInvitationScreen({
  code,
  expiresAtMs,
  onCancel,
  loading,
}: {
  code: string
  expiresAtMs: number
  onCancel: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.showInvitation' })
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [])

  const remaining = expiresAtMs - now
  const expired = remaining <= 0
  const display = useMemo(() => formatInvitationCode(code), [code])

  return (
    <ScreenShell
      title={t('title')}
      subtitle={t('subtitle')}
      hint={expired ? t('hintExpired') : t('hint')}
      footer={
        <Button variant="outline" onClick={onCancel} disabled={loading}>
          {loading ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <XCircle className="mr-2 h-4 w-4" />
          )}
          {t('actions.cancel')}
        </Button>
      }
      centered
    >
      <div className="mt-8 flex flex-col items-center gap-6 sm:mt-10">
        <div className="rounded-xl border border-border/50 bg-muted/30 px-6 py-5 font-mono text-3xl font-semibold tracking-[0.4em] text-foreground sm:text-4xl">
          {display}
        </div>
        <div
          className={cn(
            'text-sm tabular-nums',
            expired ? 'text-destructive' : 'text-muted-foreground'
          )}
        >
          {expired ? t('expired') : t('expiresIn', { remaining: formatRemaining(remaining) })}
        </div>
      </div>
    </ScreenShell>
  )
}

// ── S4 — Redeem invitation ──────────────────────────────────────────────────

function redeemErrorMessage(
  t: (k: string) => string,
  kind: RedeemInvitationErrorKind | null
): string | null {
  switch (kind) {
    case null:
      return null
    case 'invitation_not_found':
      return t('errors.invitationNotFound')
    case 'invitation_expired':
      return t('errors.invitationExpired')
    case 'passphrase_mismatch':
      return t('errors.passphraseMismatch')
    case 'sponsor_unreachable':
      return t('errors.sponsorUnreachable')
    case 'sponsor_rejected':
      return t('errors.sponsorRejected')
    case 'sponsor_declined':
      return t('errors.sponsorDeclined')
    case 'timeout':
      return t('errors.timeout')
    case 'connection_lost':
      return t('errors.connectionLost')
    case 'service_unavailable':
      return t('errors.serviceUnavailable')
    case 'device_name_required':
      return t('errors.deviceNameRequired')
    case 'internal':
    default:
      return t('errors.generic')
  }
}

export function RedeemInvitationScreen({
  onSubmit,
  onBack,
  loading,
}: {
  onSubmit: (input: {
    code: string
    passphrase: string
  }) => Promise<
    | { ok: true; redeem: RedeemResponse }
    | { ok: false; kind: RedeemInvitationErrorKind; raw: string }
  >
  onBack: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.redeemInvitation' })
  const [code, setCode] = useState('')
  const [pass, setPass] = useState('')
  const [showPass, setShowPass] = useState(false)
  const [errorKind, setErrorKind] = useState<RedeemInvitationErrorKind | null>(null)
  const passInputRef = useRef<HTMLInputElement>(null)

  const errorMessage = redeemErrorMessage(t, errorKind)
  const codeComplete = code.length === INVITATION_CODE_LENGTH
  const canSubmit = codeComplete && pass.length > 0 && !loading
  const codeInvalid = errorKind === 'invitation_not_found' || errorKind === 'invitation_expired'

  // Hand focus over to passphrase the moment the code reaches full length —
  // works for both paste and the last keystroke of manual entry.
  useEffect(() => {
    if (codeComplete) passInputRef.current?.focus()
  }, [codeComplete])

  const handleSubmit = async () => {
    setErrorKind(null)
    if (!canSubmit) return
    const res = await onSubmit({ code, passphrase: pass })
    if (!res.ok) {
      setErrorKind(res.kind)
      // 邀请码已废类——原 code 必然 404,清掉让用户必须输入新邀请码。
      // 口令错——保留 code,清 pass,焦点跳回口令框。
      if (
        res.kind === 'invitation_not_found' ||
        res.kind === 'invitation_expired' ||
        res.kind === 'sponsor_rejected'
      ) {
        setCode('')
        setPass('')
      } else if (res.kind === 'passphrase_mismatch') {
        setPass('')
        passInputRef.current?.focus()
      }
    }
  }

  return (
    <ScreenShell
      title={t('title')}
      error={errorMessage}
      hint={t('hint')}
      footer={
        <div className="flex w-full items-center justify-between gap-3">
          <Button
            variant="ghost"
            data-testid="setup-redeem-back"
            onClick={onBack}
            disabled={loading}
          >
            <ArrowLeft className="mr-2 h-4 w-4" />
            {t('actions.back')}
          </Button>
          <Button
            data-testid="setup-redeem-submit"
            onClick={handleSubmit}
            disabled={!canSubmit}
            className="min-w-32"
          >
            {loading ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                {t('actions.joining')}
              </>
            ) : (
              t('actions.submit')
            )}
          </Button>
        </div>
      }
      centered
    >
      <div className="mx-auto mt-10 w-full max-w-sm space-y-1 sm:mt-12">
        <Label htmlFor="join-code" className="sr-only">
          {t('labels.code')}
        </Label>
        <div data-testid="setup-redeem-code">
          <InvitationCodeInput
            id="join-code"
            value={code}
            onChange={setCode}
            disabled={loading}
            invalid={codeInvalid}
            autoFocus
          />
        </div>

        <AnimatePresence initial={false}>
          {codeComplete && (
            <motion.div
              key="passphrase"
              initial={{ opacity: 0, height: 0, y: -4 }}
              animate={{ opacity: 1, height: 'auto', y: 0 }}
              exit={{ opacity: 0, height: 0, y: -4 }}
              transition={{ duration: 0.22, ease: [0.22, 0.61, 0.36, 1] }}
              className="overflow-hidden"
            >
              <div className="pt-6">
                <Label htmlFor="join-pass" className="sr-only">
                  {t('labels.passphrase')}
                </Label>
                <div className="relative">
                  <Input
                    id="join-pass"
                    ref={passInputRef}
                    type={showPass ? 'text' : 'password'}
                    value={pass}
                    onChange={e => setPass(e.target.value)}
                    disabled={loading}
                    className="h-11 border-0 border-b border-border/60 bg-transparent px-0 pr-10 text-center text-base shadow-none focus-visible:border-primary focus-visible:ring-0"
                    placeholder={t('placeholders.passphrase')}
                    onKeyDown={e => e.key === 'Enter' && handleSubmit()}
                  />
                  <button
                    type="button"
                    onClick={() => setShowPass(!showPass)}
                    className="absolute right-0 top-0 flex h-full items-center px-2 text-muted-foreground transition-colors hover:text-foreground"
                    tabIndex={-1}
                  >
                    {showPass ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </ScreenShell>
  )
}

// ── S5 — Pairing complete ──────────────────────────────────────────────────

export function PairingCompleteScreen({
  role,
  redeem,
  onDone,
}: {
  role: 'sponsor' | 'joiner'
  redeem?: RedeemResponse
  onDone: () => void
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.pairingComplete' })

  return (
    <ScreenShell
      title={t('title')}
      subtitle={role === 'sponsor' ? t('sponsor.subtitle') : t('joiner.subtitle')}
      footer={
        <Button onClick={onDone} className="min-w-32">
          {t('actions.done')}
        </Button>
      }
      centered
    >
      <div className="mt-8 flex flex-col items-center gap-3 sm:mt-10">
        <CheckCircle2 className="h-12 w-12 text-emerald-500" />
        {role === 'joiner' && redeem && (
          <div className="mt-2 grid gap-1 text-center text-sm text-muted-foreground">
            <div>
              <span className="font-medium text-foreground">{t('joiner.connectedTo')}</span>{' '}
              <span className="font-mono">{redeem.sponsorDeviceId}</span>
            </div>
            <div className="font-mono text-xs">{redeem.sponsorIdentityFingerprint}</div>
          </div>
        )}
      </div>
    </ScreenShell>
  )
}
