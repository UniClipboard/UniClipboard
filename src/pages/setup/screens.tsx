import { AnimatePresence, m } from 'framer-motion'
import {
  AlertCircle,
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  CheckCircle2,
  ClipboardCheck,
  Eye,
  EyeOff,
  FileUp,
  KeyRound,
  Loader2,
  type LucideIcon,
  Package,
  Shield,
  ShieldCheck,
  Smartphone,
  Wifi,
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
import { INVITATION_CODE_LENGTH, formatInvitationCode } from '@/components/invitation-code-utils'
import { InvitationCodeInput } from '@/components/InvitationCodeInput'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useConfigImport, type ConfigImportErrorKind } from '@/hooks/useConfigImport'
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
    <m.div
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
          <AlertCircle className="size-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {footer && (
        <div className={cn('mt-7 flex sm:mt-8', centered && 'justify-center')}>{footer}</div>
      )}

      {hint && <div className="mt-4 text-xs text-muted-foreground sm:mt-5">{hint}</div>}
    </m.div>
  )
}

// ── Brand panel (split-screen left rail) ────────────────────────────────────

/**
 * Persistent dark brand rail shown on the left of the setup window (≥lg). Carries
 * the wordmark, a one-line value proposition, and the trust badges. Drag-enabled
 * so the window stays movable; on macOS the traffic lights sit over its top-left,
 * so the content starts below a small spacer. Hidden below `lg`, where the right
 * pane takes the full width.
 */
export function SetupBrandPanel() {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup' })

  const badges = [
    { icon: ShieldCheck, label: t('page.badges.e2ee') },
    { icon: KeyRound, label: t('page.badges.localKeys') },
    { icon: Wifi, label: t('page.badges.lanDiscovery') },
  ]

  return (
    <aside
      data-tauri-drag-region
      className="relative hidden flex-col justify-between overflow-hidden border-r border-border bg-zinc-950 p-10 text-white lg:flex"
    >
      {/* Restrained depth: two soft white glows + a faint grid. Monochrome to
          match the app's neutral palette — no particles, no aurora. */}
      <div aria-hidden className="pointer-events-none absolute inset-0">
        <div className="absolute -left-20 -top-24 size-72 rounded-full bg-white/10 blur-[6rem]" />
        <div className="absolute -bottom-28 -right-20 size-80 rounded-full bg-white/[0.06] blur-[7rem]" />
        <div className="absolute inset-0 opacity-[0.05] [background-image:linear-gradient(rgba(255,255,255,0.7)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,0.7)_1px,transparent_1px)] [background-size:34px_34px]" />
      </div>

      {/* Wordmark — pushed down past the macOS traffic lights. */}
      <div className="relative z-10 flex items-center gap-3 pt-3">
        <div className="flex size-10 items-center justify-center rounded-xl bg-white/10 ring-1 ring-white/15 backdrop-blur">
          <ClipboardCheck className="size-5" />
        </div>
        <span className="text-base font-semibold tracking-tight">UniClipboard</span>
      </div>

      {/* Value proposition. */}
      <div className="relative z-10 space-y-3">
        <h2 className="text-2xl font-semibold leading-snug tracking-tight">
          {t('brand.headline')}
        </h2>
        <p className="max-w-xs text-sm leading-relaxed text-white/55">{t('brand.tagline')}</p>
      </div>

      {/* Trust badges. */}
      <div className="relative z-10 flex flex-col gap-2.5">
        {badges.map(({ icon: Icon, label }) => (
          <div key={label} className="flex items-center gap-2.5 text-xs text-white/55">
            <Icon className="size-4 text-white/70" />
            {label}
          </div>
        ))}
      </div>
    </aside>
  )
}

// ── S0 — Entry ─────────────────────────────────────────────────────────────

/**
 * A single getting-started choice, rendered as a settings-style list row:
 * leading tinted icon, title + description, trailing arrow that nudges on hover.
 */
function EntryRow({
  icon: Icon,
  title,
  description,
  onClick,
  loading,
  testId,
}: {
  icon: LucideIcon
  title: string
  description: string
  onClick: () => void
  loading?: boolean
  testId: string
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      onClick={onClick}
      disabled={loading}
      className="group flex w-full items-center gap-4 px-5 py-4 text-left transition-colors hover:bg-muted/60 disabled:pointer-events-none disabled:opacity-50"
    >
      <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary transition-colors group-hover:bg-primary/15">
        <Icon className="size-5" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium text-foreground">{title}</div>
        <div className="mt-0.5 text-xs leading-relaxed text-muted-foreground">{description}</div>
      </div>
      <ArrowRight className="size-4 shrink-0 text-muted-foreground/40 transition-all group-hover:translate-x-0.5 group-hover:text-foreground" />
    </button>
  )
}

export function EntryScreen({
  onCreate,
  onJoin,
  onImport,
  loading,
}: {
  onCreate: () => void
  onJoin: () => void
  onImport: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })

  return (
    <m.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="w-full"
    >
      <div className="mb-7">
        <h1 className="text-2xl font-semibold tracking-tight text-foreground">{t('title')}</h1>
        <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{t('subtitle')}</p>
      </div>

      <div className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-card">
        <EntryRow
          icon={Shield}
          title={t('create.title')}
          description={t('create.description')}
          onClick={onCreate}
          loading={loading}
          testId="setup-entry-create"
        />
        <EntryRow
          icon={Smartphone}
          title={t('join.title')}
          description={t('join.description')}
          onClick={onJoin}
          loading={loading}
          testId="setup-entry-join"
        />
        <EntryRow
          icon={Package}
          title={t('import.title')}
          description={t('import.description')}
          onClick={onImport}
          loading={loading}
          testId="setup-entry-import"
        />
      </div>

      <p className="mt-6 text-xs leading-relaxed text-muted-foreground">{t('footer')}</p>
    </m.div>
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
            <ArrowLeft className="mr-2 size-4" />
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
                <Loader2 className="mr-2 size-4 animate-spin" />
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
              {showPass1 ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
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
              {showPass2 ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
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
            <Loader2 className="mr-2 size-4 animate-spin" />
          ) : (
            <XCircle className="mr-2 size-4" />
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
            <ArrowLeft className="mr-2 size-4" />
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
                <Loader2 className="mr-2 size-4 animate-spin" />
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
            <m.div
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
                    {showPass ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                  </button>
                </div>
              </div>
            </m.div>
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
        <CheckCircle2 className="size-12 text-emerald-500" />
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

// ── S6 — Import configuration ───────────────────────────────────────────────

/** Trailing path segment of an absolute file path (POSIX or Windows). */
function baseName(path: string): string {
  const parts = path.split(/[/\\]/)
  return parts[parts.length - 1] || path
}

/**
 * First-run "migrate from a backup" path: pick an exported `.ucbundle`, unlock
 * it with the source device's space passphrase, confirm the device-identity
 * move, then stage + restart. Mirrors the settings import flow
 * (`ConfigBackupGroup`) but rendered as a full-screen setup step with softer,
 * fresh-install copy — on an uninitialized device there is nothing to replace.
 */
export function ImportConfigScreen({ onBack }: { onBack: () => void }) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.importConfig' })
  const [showPass, setShowPass] = useState(false)
  const [errorKind, setErrorKind] = useState<ConfigImportErrorKind | null>(null)

  const imp = useConfigImport({ onError: setErrorKind })

  const errorMessage = errorKind ? t(`errors.${errorKind}`) : null
  const fileName = imp.sourcePath ? baseName(imp.sourcePath) : null

  const handlePick = () => {
    setErrorKind(null)
    void imp.pickFile()
  }
  const handleContinue = () => {
    setErrorKind(null)
    void imp.submitPassword()
  }
  const handleConfirm = () => {
    setErrorKind(null)
    void imp.confirmImport()
  }

  const sourceModeLabel = (mode: string) =>
    mode === 'portable'
      ? t('metaSourcePortable')
      : mode === 'installed'
        ? t('metaSourceInstalled')
        : mode

  // ── Restarting: forced terminal state, no navigation. ──
  if (imp.isRestarting) {
    return (
      <ScreenShell title={t('restartingTitle')} centered>
        <div className="mt-8 flex flex-col items-center gap-3 text-center sm:mt-10">
          <Loader2 className="size-8 animate-spin text-primary" />
          <p className="max-w-md text-sm text-muted-foreground">{t('restartingDescription')}</p>
          {imp.stagedResult?.unlockRequiredAfterApply && (
            <p className="text-xs text-muted-foreground">{t('restartingUnlockHint')}</p>
          )}
        </div>
      </ScreenShell>
    )
  }

  // ── Confirm: preview metadata + device-move note. ──
  if (imp.phase === 'confirm') {
    return (
      <ScreenShell
        title={t('title')}
        subtitle={t('confirmSubtitle')}
        error={errorMessage}
        footer={
          <div className="flex w-full items-center justify-between gap-3">
            <Button variant="ghost" onClick={imp.back} disabled={imp.busy}>
              <ArrowLeft className="mr-2 size-4" />
              {t('actions.back')}
            </Button>
            <Button onClick={handleConfirm} disabled={imp.busy} className="min-w-32">
              {imp.busy ? (
                <>
                  <Loader2 className="mr-2 size-4 animate-spin" />
                  {t('actions.staging')}
                </>
              ) : (
                t('actions.import')
              )}
            </Button>
          </div>
        }
      >
        <div className="mt-6 space-y-4 sm:mt-8">
          <div className="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-xs leading-snug text-foreground/90">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" />
            <span>{t('note')}</span>
          </div>

          {imp.preview && (
            <div className="space-y-1.5">
              <div className="text-xs font-medium text-muted-foreground">{t('metaTitle')}</div>
              <dl className="space-y-1 text-xs">
                <div className="flex justify-between gap-4">
                  <dt className="text-muted-foreground">{t('metaAppVersion')}</dt>
                  <dd className="tabular-nums">{imp.preview.appVersion}</dd>
                </div>
                <div className="flex justify-between gap-4">
                  <dt className="text-muted-foreground">{t('metaSourceMode')}</dt>
                  <dd>{sourceModeLabel(imp.preview.sourceMode)}</dd>
                </div>
                <div className="flex justify-between gap-4">
                  <dt className="text-muted-foreground">{t('metaFingerprint')}</dt>
                  <dd className="max-w-56 truncate font-mono">{imp.preview.deviceFingerprint}</dd>
                </div>
              </dl>
            </div>
          )}
        </div>
      </ScreenShell>
    )
  }

  // ── Idle / password: choose a bundle and unlock it. ──
  const canContinue = !!imp.sourcePath && !!imp.password && !imp.busy

  return (
    <ScreenShell
      title={t('title')}
      subtitle={t('subtitle')}
      error={errorMessage}
      footer={
        <div className="flex w-full items-center justify-between gap-3">
          <Button variant="ghost" onClick={onBack} disabled={imp.busy}>
            <ArrowLeft className="mr-2 size-4" />
            {t('actions.back')}
          </Button>
          <Button onClick={handleContinue} disabled={!canContinue} className="min-w-32">
            {imp.busy ? (
              <>
                <Loader2 className="mr-2 size-4 animate-spin" />
                {t('actions.staging')}
              </>
            ) : (
              t('actions.continue')
            )}
          </Button>
        </div>
      }
    >
      <div className="mt-6 space-y-6 sm:mt-8">
        <div className="space-y-2">
          <Label>{t('fileLabel')}</Label>
          <button
            type="button"
            data-testid="setup-import-pick"
            onClick={handlePick}
            disabled={imp.busy}
            className="flex w-full items-center gap-3 rounded-lg border border-border/60 bg-muted/30 px-4 py-3 text-left transition-colors hover:border-primary/50 hover:bg-muted/50 disabled:opacity-50"
          >
            <FileUp className="size-5 shrink-0 text-muted-foreground" />
            {fileName ? (
              <span className="min-w-0 flex-1 truncate text-sm text-foreground">{fileName}</span>
            ) : (
              <span className="flex-1 text-sm text-muted-foreground">{t('chooseFile')}</span>
            )}
            {fileName && (
              <span className="shrink-0 text-xs font-medium text-primary">{t('changeFile')}</span>
            )}
          </button>
        </div>

        <AnimatePresence initial={false}>
          {imp.sourcePath && (
            <m.div
              key="import-pass"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.22, ease: [0.22, 0.61, 0.36, 1] }}
              className="overflow-hidden"
            >
              <div className="space-y-2 pt-0.5">
                <Label htmlFor="import-pass">{t('passwordLabel')}</Label>
                <div className="relative">
                  <Input
                    id="import-pass"
                    type={showPass ? 'text' : 'password'}
                    value={imp.password}
                    onChange={e => {
                      setErrorKind(null)
                      imp.setPassword(e.target.value)
                    }}
                    disabled={imp.busy}
                    className="pr-10"
                    placeholder={t('passwordPlaceholder')}
                    onKeyDown={e => e.key === 'Enter' && canContinue && handleContinue()}
                  />
                  <button
                    type="button"
                    onClick={() => setShowPass(!showPass)}
                    className="absolute right-0 top-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground"
                  >
                    {showPass ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                  </button>
                </div>
                <p className="text-xs text-muted-foreground">{t('passwordHint')}</p>
              </div>
            </m.div>
          )}
        </AnimatePresence>
      </div>
    </ScreenShell>
  )
}
