import { AnimatePresence, m } from 'framer-motion'
import {
  AlertCircle,
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  Check,
  ChevronRight,
  FileUp,
  Globe,
  Loader2,
  type LucideIcon,
  Monitor,
  Package,
  Shield,
  ShieldCheck,
  Smartphone,
  X,
  XCircle,
} from 'lucide-react'
import { useRef, useState, type FormEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { JoinSpaceEndReason } from '@/api/daemon/setupV2'
import { AppStateShell } from '@/components/app/AppStateShell'
import { ConfigImportReplacementDialog } from '@/components/config-import/ConfigImportReplacementDialog'
import { FullInvitationInput } from '@/components/FullInvitationInput'
import { InvitationCodeInput } from '@/components/InvitationCodeInput'
import { LegacyCopyFullInvitationButton } from '@/components/LegacyCopyFullInvitationButton'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { PasswordInput } from '@/components/ui/password-input'
import { Progress } from '@/components/ui/progress'
import { useConfigImport, type ConfigImportErrorKind } from '@/hooks/useConfigImport'
import { INVITATION_CODE_LENGTH, shortInvitationInput } from '@/lib/invitation-code'
import { cn } from '@/lib/utils'
import { useInitializeSpaceForm, type InitializeSpaceSubmit } from './useInitializeSpaceForm'
import { useInvitationCountdown } from './useInvitationCountdown'
import { useRedeemInvitationForm, type RedeemInvitationSubmit } from './useRedeemInvitationForm'
import { useSpaceReadyInvite, type IssueInvitation } from './useSpaceReadyInvite'

// ── Common shell ───────────────────────────────────────────────────────────

const REVEAL_TRANSITION = { duration: 0.22, ease: [0.22, 0.61, 0.36, 1] } as const

function ScreenShell({
  category,
  categoryIcon,
  categoryTone,
  title,
  subtitle,
  children,
  footer,
  hint,
  error,
}: {
  category?: string
  categoryIcon?: ReactNode
  categoryTone?: 'default' | 'destructive' | 'success'
  title: string
  subtitle?: string
  children?: ReactNode
  footer?: ReactNode
  hint?: ReactNode
  error?: string | null
}) {
  return (
    <AppStateShell
      category={category}
      categoryIcon={categoryIcon}
      categoryTone={categoryTone}
      title={title}
      description={subtitle}
    >
      {children}
      {error && <FieldError className="mt-4">{error}</FieldError>}
      {footer && <div className="mt-7 flex flex-wrap items-center gap-2">{footer}</div>}
      {hint && <div className="mt-4 text-ui-caption-relaxed text-muted-foreground">{hint}</div>}
    </AppStateShell>
  )
}

function FieldError({
  id,
  className,
  children,
}: {
  id?: string
  className?: string
  children: ReactNode
}) {
  return (
    <p
      id={id}
      role="alert"
      className={cn(
        'flex items-start gap-1.5 text-ui-body font-medium text-destructive',
        className
      )}
    >
      <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      <span>{children}</span>
    </p>
  )
}

/** Back on the leading edge, the primary action on the trailing edge. */
function StepFooter({
  back,
  primary,
}: {
  back: {
    label: string
    onClick: () => void
    disabled?: boolean
    testId?: string
  }
  primary: ReactNode
}) {
  return (
    <div className="flex w-full items-center justify-between gap-3">
      <Button
        type="button"
        variant="ghost"
        className="-ml-2.5"
        data-testid={back.testId}
        onClick={back.onClick}
        disabled={back.disabled}
      >
        <ArrowLeft aria-hidden="true" />
        {back.label}
      </Button>
      {primary}
    </div>
  )
}

function BusyLabel({
  busy,
  busyLabel,
  label,
}: {
  busy: boolean
  busyLabel: string
  label: string
}) {
  return (
    <>
      {busy && <Loader2 className="animate-spin" aria-hidden="true" />}
      {busy ? busyLabel : label}
    </>
  )
}

const iconProps = { className: 'size-4', 'aria-hidden': true } as const

// ── S0 — Entry ─────────────────────────────────────────────────────────────

/** One getting-started choice rendered as a list row with a trailing chevron. */
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
      className="group flex w-full items-center gap-3.5 px-4 py-3.5 text-left transition-colors outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:ring-inset disabled:pointer-events-none disabled:opacity-50"
    >
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted text-foreground transition-colors group-hover:bg-background">
        <Icon {...iconProps} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-ui-body font-medium text-foreground">{title}</span>
        <span className="mt-0.5 block text-ui-caption-relaxed text-muted-foreground">
          {description}
        </span>
      </span>
      <ChevronRight
        className="size-4 shrink-0 text-muted-foreground transition-colors group-hover:text-foreground"
        aria-hidden="true"
      />
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
    <ScreenShell title={t('title')} subtitle={t('subtitle')}>
      <div className="mt-7 flex flex-col divide-y divide-border overflow-hidden rounded-xl border border-border bg-card">
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
      </div>

      <section aria-labelledby="setup-entry-import-heading" className="mt-6">
        <h2
          id="setup-entry-import-heading"
          className="mb-2 text-ui-caption font-medium text-muted-foreground"
        >
          {t('import.divider')}
        </h2>
        <div className="overflow-hidden rounded-xl border border-border bg-card">
          <EntryRow
            icon={Package}
            title={t('import.title')}
            description={t('import.description')}
            onClick={onImport}
            loading={loading}
            testId="setup-entry-import"
          />
        </div>
      </section>

      <p className="mt-6 flex items-start gap-2 text-ui-caption-relaxed text-muted-foreground">
        <ShieldCheck className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        <span>{t('footer')}</span>
      </p>
    </ScreenShell>
  )
}

// ── S1 — Initialize space ───────────────────────────────────────────────────

export function InitializeSpaceScreen({
  onSubmit,
  onBack,
  loading,
}: {
  onSubmit: InitializeSpaceSubmit
  onBack: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.initializeSpace' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  const form = useInitializeSpaceForm(onSubmit, loading)
  const { deviceName, pass1, pass2, deviceNameError, passphraseError, generalError } = form

  return (
    <ScreenShell
      category={tWelcome('create.cta')}
      categoryIcon={<Shield {...iconProps} />}
      title={t('title')}
      subtitle={t('subtitle')}
      error={generalError}
      footer={
        <StepFooter
          back={{
            label: t('actions.back'),
            onClick: onBack,
            disabled: loading,
            testId: 'setup-initialize-back',
          }}
          primary={
            <Button
              type="submit"
              form="setup-initialize-form"
              data-testid="setup-initialize-submit"
              disabled={loading}
            >
              <BusyLabel
                busy={Boolean(loading)}
                busyLabel={t('actions.creating')}
                label={t('actions.submit')}
              />
            </Button>
          }
        />
      }
    >
      <form
        id="setup-initialize-form"
        onSubmit={event => void form.submit(event)}
        className="mt-6 flex flex-col gap-5"
      >
        <div className="flex flex-col gap-2">
          <Label htmlFor="device-name">{t('labels.deviceName')}</Label>
          <Input
            id="device-name"
            value={deviceName}
            onChange={e => form.setDeviceName(e.target.value)}
            disabled={loading}
            placeholder={t('placeholders.deviceName')}
            aria-invalid={deviceNameError !== null}
            aria-describedby={deviceNameError ? 'device-name-error' : undefined}
          />
          {deviceNameError && <FieldError id="device-name-error">{deviceNameError}</FieldError>}
        </div>

        <div className="flex flex-col gap-2">
          <Label htmlFor="pass1">{t('labels.passphrase')}</Label>
          <PasswordInput
            id="pass1"
            value={pass1}
            onChange={e => form.setPass1(e.target.value)}
            disabled={loading}
            placeholder={t('placeholders.passphrase')}
            aria-describedby="pass1-help"
          />
          <p id="pass1-help" className="text-ui-caption-relaxed text-muted-foreground">
            {t('hint')}
          </p>
        </div>

        <div className="flex flex-col gap-2">
          <Label htmlFor="pass2">{t('labels.passphraseConfirm')}</Label>
          <PasswordInput
            id="pass2"
            value={pass2}
            onChange={e => form.setPass2(e.target.value)}
            disabled={loading}
            placeholder={t('placeholders.passphraseConfirm')}
            aria-invalid={passphraseError !== null}
            aria-describedby={passphraseError ? 'pass2-error' : undefined}
          />
          {passphraseError && <FieldError id="pass2-error">{passphraseError}</FieldError>}
        </div>
      </form>
    </ScreenShell>
  )
}

// ── S3 — Show invitation ────────────────────────────────────────────────────

export function ShowInvitationScreen({
  code,
  fullInvitation,
  expiresAtMs,
  onCancel,
  loading,
}: {
  code: string
  fullInvitation?: string
  expiresAtMs: number
  onCancel: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.showInvitation' })
  const { display, expired, progress, remainingLabel } = useInvitationCountdown(
    code,
    expiresAtMs,
    onCancel
  )

  return (
    <ScreenShell
      category={t('status')}
      categoryIcon={<Check {...iconProps} />}
      categoryTone="success"
      title={t('title')}
      subtitle={t('subtitle')}
      hint={expired ? t('hintExpired') : t('hint')}
      footer={
        <Button
          variant="outline"
          data-testid="setup-invitation-cancel"
          onClick={onCancel}
          disabled={loading}
        >
          {loading ? <Loader2 className="animate-spin" aria-hidden="true" /> : <X aria-hidden />}
          {t('actions.cancel')}
        </Button>
      }
    >
      <div className="mt-7 flex flex-col gap-4 rounded-xl border border-border bg-muted/50 p-5">
        <div
          data-testid="setup-invitation-code"
          className={cn(
            'select-all break-words text-center font-mono text-ui-title font-semibold tabular-nums',
            expired && 'text-muted-foreground line-through decoration-1'
          )}
        >
          {display}
        </div>
        <div className="flex flex-col gap-2">
          <Progress value={progress} className="h-1" aria-hidden="true" />
          <p
            role="timer"
            className={cn(
              'text-ui-caption tabular-nums',
              expired ? 'text-destructive' : 'text-muted-foreground'
            )}
          >
            {expired ? t('expired') : t('expiresIn', { remaining: remainingLabel })}
          </p>
        </div>
        {fullInvitation && (
          <LegacyCopyFullInvitationButton
            invitation={fullInvitation}
            disabled={expired || loading}
          />
        )}
      </div>
    </ScreenShell>
  )
}

// ── S4 — Redeem invitation ──────────────────────────────────────────────────

export function RedeemInvitationScreen({
  onSubmit,
  onBack,
  loading,
}: {
  onSubmit: RedeemInvitationSubmit
  onBack: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.redeemInvitation' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  const {
    code,
    setCode,
    pass,
    setPass,
    codeComplete,
    canSubmit,
    codeInvalid,
    errorMessage,
    submit,
  } = useRedeemInvitationForm(onSubmit, loading)
  const passInputRef = useRef<HTMLInputElement>(null)

  return (
    <ScreenShell
      category={tWelcome('join.cta')}
      categoryIcon={<Smartphone {...iconProps} />}
      title={t('title')}
      subtitle={t('subtitle')}
      error={errorMessage}
      footer={
        <StepFooter
          back={{
            label: t('actions.back'),
            onClick: onBack,
            disabled: loading,
            testId: 'setup-redeem-back',
          }}
          primary={
            <Button
              type="submit"
              form="setup-redeem-form"
              data-testid="setup-redeem-submit"
              disabled={!canSubmit}
            >
              <BusyLabel
                busy={Boolean(loading)}
                busyLabel={t('actions.joining')}
                label={t('actions.submit')}
              />
            </Button>
          }
        />
      }
    >
      <form
        id="setup-redeem-form"
        onSubmit={event => void submit(event)}
        className="mt-6 flex flex-col gap-5"
      >
        <div className="flex flex-col gap-2">
          <Label htmlFor="join-code">{t('labels.code')}</Label>
          <div data-testid="setup-redeem-code" className="w-fit">
            <InvitationCodeInput
              id="join-code"
              value={shortInvitationInput(code)}
              onChange={value => {
                setCode(value)
                if (value.length === INVITATION_CODE_LENGTH) passInputRef.current?.focus()
              }}
              disabled={loading}
              invalid={codeInvalid}
              autoFocus
            />
          </div>
        </div>

        <FullInvitationInput
          id="join-full-invitation"
          value={code}
          onChange={setCode}
          disabled={loading}
        />
        <AnimatePresence initial={false}>
          {codeComplete && (
            <m.div
              key="passphrase"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              transition={REVEAL_TRANSITION}
              className="-m-1 overflow-hidden p-1"
            >
              <div className="flex flex-col gap-2">
                <Label htmlFor="join-pass">{t('labels.passphrase')}</Label>
                <PasswordInput
                  id="join-pass"
                  ref={passInputRef}
                  autoFocus
                  value={pass}
                  onChange={e => setPass(e.target.value)}
                  disabled={loading}
                  placeholder={t('placeholders.passphrase')}
                />
              </div>
            </m.div>
          )}
        </AnimatePresence>

        <p className="text-ui-caption-relaxed text-muted-foreground">{t('hint')}</p>
      </form>
    </ScreenShell>
  )
}

export function JoinPendingScreen({
  status,
  onCancel,
  loading = false,
}: {
  status: 'pending' | 'processing'
  onCancel: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.joinPending' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  return (
    <ScreenShell
      category={tWelcome('join.cta')}
      categoryIcon={
        <Loader2 className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
      }
      title={t(status === 'processing' ? 'processing.title' : 'title')}
      subtitle={t(status === 'processing' ? 'processing.subtitle' : 'subtitle')}
      footer={
        status === 'pending' ? (
          <Button
            data-testid="setup-join-cancel"
            variant="outline"
            onClick={onCancel}
            disabled={loading}
          >
            {t('actions.cancel')}
          </Button>
        ) : undefined
      }
    >
      <div
        data-testid={status === 'processing' ? 'setup-join-processing' : 'setup-join-pending'}
        aria-hidden="true"
        className="mt-8 h-1.5 overflow-hidden rounded-full bg-muted"
      >
        <div className="h-full w-1/3 animate-pulse rounded-full bg-primary/35 motion-reduce:animate-none" />
      </div>
    </ScreenShell>
  )
}

export function JoinEndedScreen({
  reason,
  onBack,
}: {
  reason: JoinSpaceEndReason
  onBack: () => void
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.joinPending' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  return (
    <ScreenShell
      category={tWelcome('join.cta')}
      categoryIcon={<XCircle {...iconProps} />}
      categoryTone="destructive"
      title={t('ended.title')}
      footer={
        <Button data-testid="setup-join-ended-back" onClick={onBack}>
          {t('actions.back')}
        </Button>
      }
    >
      <p
        data-testid="setup-join-ended"
        role="alert"
        className="mt-2 text-ui-body-relaxed text-foreground"
      >
        {t(`ended.reasons.${reason}`)}
      </p>
    </ScreenShell>
  )
}

// ── Sponsor Space ready ────────────────────────────────────────────────────

export function SpaceReadyScreen({
  onInvite,
  onDone,
  loading = false,
}: {
  onInvite: IssueInvitation
  onDone: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.spaceReady' })
  const { t: tStatus } = useTranslation(undefined, { keyPrefix: 'setup.showInvitation' })
  const { inviteError, inviteRetryable, invite } = useSpaceReadyInvite(onInvite)

  return (
    <ScreenShell
      category={tStatus('status')}
      categoryIcon={<Check {...iconProps} />}
      categoryTone="success"
      title={t('title')}
      subtitle={t('subtitle')}
      error={inviteError}
      footer={
        <>
          <Button
            data-testid="setup-complete-invite"
            onClick={() => void invite()}
            disabled={loading || !inviteRetryable}
          >
            {loading ? (
              <Loader2 className="animate-spin" aria-hidden="true" />
            ) : (
              <Monitor aria-hidden="true" />
            )}
            {loading ? t('actions.inviting') : t('actions.invite')}
          </Button>
          <Button
            data-testid="setup-complete-later"
            variant="ghost"
            onClick={onDone}
            disabled={loading}
          >
            {t('actions.later')}
          </Button>
        </>
      }
    />
  )
}

// ── S5 — Pairing complete ──────────────────────────────────────────────────

function shortDeviceId(deviceId?: string | null): string {
  if (!deviceId) return '---'
  if (deviceId.length <= 14) return deviceId
  return `${deviceId.slice(0, 6)}...${deviceId.slice(-4)}`
}

export function PairingCompleteScreen({
  localDeviceName,
  peerDeviceId,
  onDone,
}: {
  localDeviceName?: string | null
  peerDeviceId?: string | null
  onDone: () => void
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.pairingComplete' })

  return (
    <ScreenShell
      category={t('connected')}
      categoryIcon={<Check {...iconProps} />}
      categoryTone="success"
      title={t('title')}
      footer={
        <Button data-testid="setup-complete-done" onClick={onDone}>
          {t('actions.done')}
          <ArrowRight aria-hidden="true" />
        </Button>
      }
    >
      <ul
        data-testid="setup-pairing-complete"
        className="mt-6 flex flex-col divide-y divide-border rounded-xl border border-border bg-card"
      >
        <li className="flex items-center gap-3 px-4 py-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
            <Monitor {...iconProps} />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block break-words text-ui-body font-medium">
              {localDeviceName || t('devices.thisDevice')}
            </span>
            <span className="block text-ui-caption text-muted-foreground">
              {t('devices.local')}
            </span>
          </span>
        </li>
        <li className="flex items-center gap-3 px-4 py-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
            <Monitor {...iconProps} />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-ui-body font-medium">{t('devices.peer')}</span>
            <span
              data-testid="setup-complete-peer-id"
              className="block truncate font-mono text-ui-caption text-muted-foreground"
            >
              {shortDeviceId(peerDeviceId)}
            </span>
          </span>
          <span className="shrink-0 rounded-full border border-border px-2 py-0.5 text-ui-caption text-muted-foreground">
            {t('devices.joined')}
          </span>
        </li>
      </ul>

      <ul className="mt-5 flex flex-col gap-2.5 text-ui-body text-muted-foreground">
        <li className="flex items-start gap-2.5">
          <Shield className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span>{t('points.encrypted')}</span>
        </li>
        <li className="flex items-start gap-2.5">
          <Globe className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span>{t('points.sync')}</span>
        </li>
      </ul>
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
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  const [errorKind, setErrorKind] = useState<ConfigImportErrorKind | null>(null)

  const imp = useConfigImport({ onError: setErrorKind })

  const { t: tRecovery } = useTranslation(undefined, { keyPrefix: 'configImportRecovery' })
  const errorMessage =
    errorKind === 'statusRead'
      ? tRecovery('statusReadFailed')
      : errorKind
        ? t(`errors.${errorKind}`)
        : null
  const fileName = imp.sourcePath ? baseName(imp.sourcePath) : null
  const category = tWelcome('import.title')
  const categoryIcon = <Package {...iconProps} />

  const handlePick = () => {
    setErrorKind(null)
    void imp.pickFile()
  }
  const handleContinue = (event: FormEvent) => {
    event.preventDefault()
    if (!canContinue) return
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
      <ScreenShell
        category={category}
        categoryIcon={
          <Loader2 className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
        }
        title={t('restartingTitle')}
        subtitle={t('restartingDescription')}
        hint={imp.stagedResult?.unlockRequiredAfterApply ? t('restartingUnlockHint') : undefined}
      />
    )
  }

  // ── Confirm: preview metadata + device-move note. ──
  if (imp.phase === 'confirm' || imp.phase === 'replaceRejected') {
    return (
      <ScreenShell
        category={category}
        categoryIcon={categoryIcon}
        title={t('title')}
        subtitle={t('confirmSubtitle')}
        error={errorMessage}
        footer={
          <StepFooter
            back={{
              label: t('actions.back'),
              onClick: imp.back,
              disabled: imp.busy,
            }}
            primary={
              <Button onClick={handleConfirm} disabled={imp.busy}>
                <BusyLabel
                  busy={imp.busy}
                  busyLabel={t('actions.staging')}
                  label={t('actions.import')}
                />
              </Button>
            }
          />
        }
      >
        <ConfigImportReplacementDialog
          open={imp.phase === 'replaceRejected'}
          busy={imp.busy}
          onConfirm={imp.confirmReplacement}
          onCancel={imp.cancelReplacement}
        />
        <div className="mt-6 flex flex-col gap-4">
          <div className="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2.5 text-ui-body">
            <AlertTriangle
              className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400"
              aria-hidden="true"
            />
            <span>{t('note')}</span>
          </div>

          {imp.preview && (
            <section aria-labelledby="import-meta-heading" className="flex flex-col gap-1.5">
              <h2
                id="import-meta-heading"
                className="text-ui-caption font-medium text-muted-foreground"
              >
                {t('metaTitle')}
              </h2>
              <dl className="flex flex-col gap-1 text-ui-caption">
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
                  <dd className="max-w-56 truncate font-mono" title={imp.preview.deviceFingerprint}>
                    {imp.preview.deviceFingerprint}
                  </dd>
                </div>
              </dl>
            </section>
          )}
        </div>
      </ScreenShell>
    )
  }

  // ── Idle / password: choose a bundle and unlock it. ──
  const canContinue = !!imp.sourcePath && !!imp.password && !imp.busy

  return (
    <ScreenShell
      category={category}
      categoryIcon={categoryIcon}
      title={t('title')}
      subtitle={t('subtitle')}
      error={errorMessage}
      footer={
        <StepFooter
          back={{
            label: t('actions.back'),
            onClick: onBack,
            disabled: imp.busy,
          }}
          primary={
            <Button type="submit" form="setup-import-form" disabled={!canContinue}>
              <BusyLabel
                busy={imp.busy}
                busyLabel={t('actions.staging')}
                label={t('actions.continue')}
              />
            </Button>
          }
        />
      }
    >
      <form id="setup-import-form" onSubmit={handleContinue} className="mt-6 flex flex-col gap-5">
        <div className="flex flex-col gap-2">
          <span id="setup-import-file-label" className="text-ui-body font-medium">
            {t('fileLabel')}
          </span>
          <button
            type="button"
            data-testid="setup-import-pick"
            aria-labelledby="setup-import-file-label setup-import-file-value"
            onClick={handlePick}
            disabled={imp.busy}
            className="flex w-full items-center gap-3 rounded-lg border border-input px-3 py-2.5 text-left transition-colors outline-none hover:bg-muted focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50"
          >
            <FileUp className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span
              id="setup-import-file-value"
              className={cn(
                'min-w-0 flex-1 truncate text-ui-body',
                fileName ? 'text-foreground' : 'text-muted-foreground'
              )}
            >
              {fileName ?? t('chooseFile')}
            </span>
            {fileName && (
              <span className="shrink-0 text-ui-caption font-medium">{t('changeFile')}</span>
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
              transition={REVEAL_TRANSITION}
              className="-m-1 overflow-hidden p-1"
            >
              <div className="flex flex-col gap-2">
                <Label htmlFor="import-pass">{t('passwordLabel')}</Label>
                <PasswordInput
                  id="import-pass"
                  value={imp.password}
                  onChange={e => {
                    setErrorKind(null)
                    imp.setPassword(e.target.value)
                  }}
                  disabled={imp.busy}
                  placeholder={t('passwordPlaceholder')}
                  aria-describedby="import-pass-help"
                />
                <p id="import-pass-help" className="text-ui-caption-relaxed text-muted-foreground">
                  {t('passwordHint')}
                </p>
              </div>
            </m.div>
          )}
        </AnimatePresence>
      </form>
    </ScreenShell>
  )
}
