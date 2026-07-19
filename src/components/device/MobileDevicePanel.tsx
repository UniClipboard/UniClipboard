/**
 * MobileDevicePanel: persistent detail pane for a single mobile-sync device.
 *
 * The pane renders one of three states; parents must key it by
 * `device.deviceId` so switching devices remounts with fresh state:
 *
 *  - steady (revisit): the common case. The server only keeps a password
 *    hash, so once you navigate away the pairing QR is gone forever. This
 *    state leads with the device facts + server address and offers an
 *    honest "reset password to re-pair" call-to-action instead of an empty
 *    QR placeholder.
 *  - fresh (just created / reset): the transient moment right after
 *    `updateMobileDevice` returned a one-time password. Leads with the
 *    pairing QR and the one-time credential echo.
 *  - edit: label / username / password rotation form.
 */

import {
  AlertTriangle,
  Check,
  ChevronRight,
  Copy,
  Eye,
  EyeOff,
  KeyRound,
  Loader2,
  Pencil,
  QrCode,
  Smartphone,
  Trash2,
  Wand2,
  Wifi,
} from 'lucide-react'
import { QRCodeSVG } from 'qrcode.react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  DEFAULT_MOBILE_LAN_BIND_IP,
  DEFAULT_MOBILE_LAN_PORT,
  isMobileSyncError,
  listMobileLanInterfaces,
  updateMobileDevice,
  type LanInterfaceView,
  type MobileDeviceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
  type UpdateMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { BaseUrlChip } from '@/components/device/MobileSyncBaseUrlChip'
import { MobileSyncInstallHelper } from '@/components/device/MobileSyncInstallHelper'
import PanelFactRow from '@/components/device/PanelFactRow'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard'
import { formatRelativeTime, useNow } from '@/hooks/useRelativeTime'
import { createLogger } from '@/lib/logger'
import { buildConnectUri } from '@/lib/mobileSyncConnectUri'
import { cn } from '@/lib/utils'

const log = createLogger('mobile-device-panel')

/** A device counts as "online" if it was seen within this window. */
const ONLINE_WINDOW_MS = 10 * 60 * 1000

type View = 'info' | 'edit'
type FieldErrorKey = 'label' | 'username' | 'password'
type FieldErrors = Partial<Record<FieldErrorKey, string>>

interface Props {
  device: MobileDeviceView
  settings: MobileSyncSettingsView | null
  /**
   * One-time credentials from a *just-completed add*. Seeds the fresh state
   * (pairing QR + credential echo + install helper) inline, replacing the old
   * post-registration modal. Only consumed on mount — the panel is keyed by
   * `deviceId`, so navigating away and back remounts into the steady state and
   * the one-time secret is gone (same guarantee the modal's close gave).
   */
  initialCredential?: RegisterMobileDeviceResult | null
  onRevoke: (device: MobileDeviceView) => void
  onRotated: () => void
}

const MobileDevicePanel: React.FC<Props> = ({
  device,
  settings,
  initialCredential,
  onRevoke,
  onRotated,
}) => {
  const { t } = useTranslation()

  const [view, setView] = useState<View>('info')
  // A RegisterMobileDeviceResult is a structural superset of the update result,
  // so a just-added device seeds the same fresh-state machinery as a reset.
  const [credentialResult, setCredentialResult] = useState<UpdateMobileDeviceResult | null>(
    () => initialCredential ?? null
  )
  const [credentialFromRename, setCredentialFromRename] = useState(false)
  // The install helper QR only ships with a registration result (add flow); a
  // password reset never carries it, so it's cleared on every in-panel rotate.
  const [installQr, setInstallQr] = useState<string | null>(
    () => initialCredential?.installQrCodePngBase64 ?? null
  )

  const [labelInput, setLabelInput] = useState('')
  const [editBaseUsername, setEditBaseUsername] = useState('')
  const [usernameInput, setUsernameInput] = useState('')
  const [passwordInput, setPasswordInput] = useState('')
  const [autoGeneratePassword, setAutoGeneratePassword] = useState(false)
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({})
  const [formError, setFormError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const [lanInterfaces, setLanInterfaces] = useState<LanInterfaceView[]>([])
  const [selectedHost, setSelectedHost] = useState<string | null>(null)
  const [passwordVisible, setPasswordVisible] = useState(false)
  const { copied: backupCopied, copy: copyBackup } = useCopyToClipboard()

  useEffect(() => {
    let cancelled = false
    listMobileLanInterfaces()
      .then(list => {
        if (!cancelled) setLanInterfaces(list)
      })
      .catch(err => log.warn({ err }, 'failed to list LAN interfaces'))
    return () => {
      cancelled = true
    }
  }, [])

  const visibleLabel = credentialResult?.label ?? device.label
  const visibleUsername = credentialResult?.username ?? device.username

  const port = useMemo(() => {
    if (settings?.lanPort != null) return String(settings.lanPort)
    return String(DEFAULT_MOBILE_LAN_PORT)
  }, [settings?.lanPort])

  const preferredHost = settings?.lanAdvertiseIp ?? null

  const dropdownInterfaces = useMemo<LanInterfaceView[]>(() => {
    const seen = new Set<string>()
    const out: LanInterfaceView[] = []
    for (const iface of lanInterfaces) {
      if (!seen.has(iface.ipv4)) {
        seen.add(iface.ipv4)
        out.push(iface)
      }
    }
    if (preferredHost !== null && preferredHost !== '' && !seen.has(preferredHost)) {
      out.push({ name: preferredHost, ipv4: preferredHost })
    }
    return out
  }, [lanInterfaces, preferredHost])

  const effectiveSelectedHost =
    selectedHost ??
    (preferredHost !== null && dropdownInterfaces.some(i => i.ipv4 === preferredHost)
      ? preferredHost
      : dropdownInterfaces[0]?.ipv4) ??
    DEFAULT_MOBILE_LAN_BIND_IP

  const effectiveBaseUrl = useMemo(() => {
    return `http://${effectiveSelectedHost}:${port}`
  }, [effectiveSelectedHost, port])

  const connectUri = useMemo<string | null>(() => {
    if (!credentialResult?.password) return null
    try {
      const candidates = new Set<string>([effectiveBaseUrl])
      if (settings?.lanAdvertiseBaseUrl) candidates.add(settings.lanAdvertiseBaseUrl)
      for (const iface of dropdownInterfaces) candidates.add(`http://${iface.ipv4}:${port}`)
      return buildConnectUri(
        [...candidates],
        credentialResult.username,
        credentialResult.password,
        {
          label: credentialResult.label,
          did: credentialResult.deviceId,
          proto: 'syncclipboard',
        }
      )
    } catch (err) {
      log.warn({ err }, 'failed to build connect URI in device panel')
      return null
    }
  }, [credentialResult, dropdownInterfaces, effectiveBaseUrl, port, settings])

  const clearFieldError = useCallback((key: FieldErrorKey) => {
    setFieldErrors(prev => {
      if (prev[key] === undefined) return prev
      const next = { ...prev }
      delete next[key]
      return next
    })
  }, [])

  const handleOpenEdit = useCallback(
    // `armRegenerate` pre-selects auto-generate so the steady-state "reset
    // password to re-pair" CTA lands the user one click from a new QR.
    (armRegenerate = false) => {
      setLabelInput(visibleLabel)
      setEditBaseUsername(visibleUsername)
      setUsernameInput(visibleUsername)
      setPasswordInput('')
      setAutoGeneratePassword(armRegenerate)
      setFieldErrors({})
      setFormError(null)
      setView('edit')
    },
    [visibleLabel, visibleUsername]
  )

  const handleSubmitEdit = useCallback(async () => {
    const nextLabel = labelInput.trim()
    const nextUsername = usernameInput.trim()
    if (nextLabel === '') {
      setFieldErrors({ label: t('devices.mobileSync.errors.labelEmpty') })
      setFormError(null)
      return
    }
    setSubmitting(true)
    setFieldErrors({})
    setFormError(null)
    try {
      const password =
        passwordInput.length > 0 ? passwordInput : autoGeneratePassword ? null : undefined
      const result = await updateMobileDevice({
        deviceId: device.deviceId,
        label: nextLabel,
        username: nextUsername,
        ...(password !== undefined ? { password } : {}),
      })
      // A username rename mints a brand-new password server-side; flag it so the
      // credential echo warns that the old password is dead (vs. a plain reset).
      setCredentialFromRename(result.password != null && nextUsername !== editBaseUsername)
      setCredentialResult(result)
      // A reset re-pairs an existing device; the "install a client" helper is an
      // add-only affordance, so drop any seeded install QR.
      setInstallQr(null)
      setPasswordInput('')
      setAutoGeneratePassword(false)
      setPasswordVisible(false)
      setView('info')
      onRotated()
    } catch (err) {
      log.error({ err, deviceId: device.deviceId }, 'failed to update mobile device')
      const dispatch = classifyEditError(t, err)
      if (dispatch.kind === 'field') {
        setFieldErrors({ [dispatch.field]: dispatch.message })
      } else {
        setFormError(dispatch.message)
      }
    } finally {
      setSubmitting(false)
    }
  }, [
    autoGeneratePassword,
    device.deviceId,
    editBaseUsername,
    labelInput,
    onRotated,
    passwordInput,
    t,
    usernameInput,
  ])

  const handleBackup = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation()
      if (!credentialResult?.password) return
      const text = `Server: ${effectiveBaseUrl}\nUsername: ${credentialResult.username}\nPassword: ${credentialResult.password}`
      await copyBackup(text)
    },
    [copyBackup, credentialResult, effectiveBaseUrl]
  )

  // The one-time password only exists right after a create / reset; that's the
  // "pairing" moment, where the header speaks to the task ("connect your phone")
  // rather than presenting the device as a record.
  const pairing = view === 'info' && credentialResult?.password != null

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 px-8 py-8">
      {/* ── header ──────────────────────────────────────────────── */}
      <div className="flex items-start gap-4">
        <div className="flex size-13 shrink-0 items-center justify-center rounded-2xl bg-info/10 text-info ring-1 ring-inset ring-info/20">
          <Smartphone className="size-6" />
        </div>
        <div className="min-w-0 flex-1">
          {pairing ? (
            <>
              <h3 className="truncate text-xl font-semibold tracking-tight text-foreground">
                {t('devices.mobileSync.deviceDialog.connectTitle', { label: visibleLabel })}
              </h3>
              <p className="mt-1 text-xs text-muted-foreground">
                {t('devices.mobileSync.deviceDialog.connectSubtitle')}
              </p>
            </>
          ) : view === 'edit' ? (
            <>
              <h3 className="truncate text-xl font-semibold tracking-tight text-foreground">
                {t('devices.mobileSync.edit.title')}
              </h3>
              <p className="mt-1 text-xs text-muted-foreground">
                {t('devices.mobileSync.edit.subtitle')}
              </p>
            </>
          ) : (
            <>
              <h3 className="truncate text-xl font-semibold tracking-tight text-foreground">
                {visibleLabel}
              </h3>
              <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
                <span className="truncate font-mono text-xs text-muted-foreground">
                  {visibleUsername}
                </span>
                <DeviceStatusPill lastSeenAtMs={device.lastSeenAtMs} />
              </div>
            </>
          )}
        </div>
        {view === 'info' && (
          <div className="flex shrink-0 gap-2">
            {/* No "edit" during the pairing moment — keep the focus on scanning. */}
            {!pairing && (
              <Button variant="outline" size="sm" onClick={() => handleOpenEdit()}>
                <Pencil className="size-3.5" />
                {t('devices.mobileSync.edit.button')}
              </Button>
            )}
            <Button
              variant="outline"
              size="sm"
              className="border-destructive/30 text-destructive hover:border-destructive/50 hover:bg-destructive/10 hover:text-destructive"
              onClick={() => onRevoke(device)}
            >
              <Trash2 className="size-3.5" />
              {t('devices.mobileSync.revoke.confirm')}
            </Button>
          </div>
        )}
      </div>

      {view === 'info' ? (
        <InfoView
          device={device}
          effectiveBaseUrl={effectiveBaseUrl}
          dropdownInterfaces={dropdownInterfaces}
          port={port}
          selectedHost={effectiveSelectedHost}
          onSelectHost={setSelectedHost}
          connectUri={connectUri}
          credentialResult={credentialResult}
          credentialFromRename={credentialFromRename}
          installQr={installQr}
          passwordVisible={passwordVisible}
          setPasswordVisible={setPasswordVisible}
          backupCopied={backupCopied}
          onBackup={handleBackup}
          onRepair={() => handleOpenEdit(true)}
        />
      ) : (
        <>
          <EditView
            labelInput={labelInput}
            usernameInput={usernameInput}
            baseUsername={editBaseUsername}
            passwordInput={passwordInput}
            autoGeneratePassword={autoGeneratePassword}
            submitting={submitting}
            fieldErrors={fieldErrors}
            formError={formError}
            onLabelChange={value => {
              setLabelInput(value)
              clearFieldError('label')
            }}
            onUsernameChange={value => {
              setUsernameInput(value)
              clearFieldError('username')
            }}
            onPasswordChange={value => {
              setPasswordInput(value)
              if (value.length > 0) setAutoGeneratePassword(false)
              clearFieldError('password')
            }}
            onToggleAutoPassword={() => {
              setPasswordInput('')
              setAutoGeneratePassword(v => !v)
              clearFieldError('password')
            }}
          />
          <div className="flex justify-end gap-2 border-t border-border/50 pt-4">
            <Button variant="ghost" size="sm" onClick={() => setView('info')} disabled={submitting}>
              {t('devices.mobileSync.edit.cancel')}
            </Button>
            <Button
              size="sm"
              onClick={handleSubmitEdit}
              disabled={submitting || labelInput.trim() === '' || usernameInput.trim() === ''}
            >
              {submitting && <Loader2 className="size-4 animate-spin" />}
              {submitting ? t('devices.mobileSync.edit.saving') : t('devices.mobileSync.edit.save')}
            </Button>
          </div>
        </>
      )}
    </div>
  )
}

export default MobileDevicePanel

// ────────────────────────────────────────────────────────────────
// Header: online / offline status pill (ticks on the shared 30s clock)
// ────────────────────────────────────────────────────────────────

const DeviceStatusPill: React.FC<{ lastSeenAtMs: number | null | undefined }> = ({
  lastSeenAtMs,
}) => {
  const { t } = useTranslation()
  const now = useNow()

  if (lastSeenAtMs == null) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full bg-muted px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
        <span className="size-1.5 rounded-full bg-current opacity-60" />
        {t('devices.mobileSync.deviceDialog.status.neverSeen')}
      </span>
    )
  }

  const online = now - lastSeenAtMs <= ONLINE_WINDOW_MS
  if (online) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full bg-success/12 px-2 py-0.5 text-[11px] font-medium text-success">
        <span className="size-1.5 rounded-full bg-current" />
        {t('devices.mobileSync.deviceDialog.status.online')}
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full bg-muted px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
      <span className="size-1.5 rounded-full bg-current opacity-60" />
      {t('devices.mobileSync.deviceDialog.status.offline')} ·{' '}
      {formatRelativeTime(lastSeenAtMs, now, t)}
    </span>
  )
}

// ────────────────────────────────────────────────────────────────
// Info view — steady vs. fresh (one-time credentials present)
// ────────────────────────────────────────────────────────────────

interface InfoViewProps {
  device: MobileDeviceView
  effectiveBaseUrl: string
  dropdownInterfaces: LanInterfaceView[]
  port: string
  selectedHost: string | null
  onSelectHost: (host: string) => void
  connectUri: string | null
  credentialResult: UpdateMobileDeviceResult | null
  credentialFromRename: boolean
  installQr: string | null
  passwordVisible: boolean
  setPasswordVisible: (v: boolean) => void
  backupCopied: boolean
  onBackup: (e: React.MouseEvent) => void
  onRepair: () => void
}

const InfoView: React.FC<InfoViewProps> = ({
  device,
  effectiveBaseUrl,
  dropdownInterfaces,
  port,
  selectedHost,
  onSelectHost,
  connectUri,
  credentialResult,
  credentialFromRename,
  installQr,
  passwordVisible,
  setPasswordVisible,
  backupCopied,
  onBackup,
  onRepair,
}) => {
  const { t } = useTranslation()
  // The plaintext password only exists in the brief window after a create /
  // reset. That's the only time we can render a pairing QR.
  const hasCredential = credentialResult?.password != null

  const addressChip = (
    <BaseUrlChip
      baseUrl={effectiveBaseUrl}
      interfaces={dropdownInterfaces}
      port={port}
      selectedHost={selectedHost}
      onSelect={onSelectHost}
    />
  )

  if (hasCredential) {
    // Guided pairing: the QR is the one thing to do. Everything else — install
    // the app, or type the credentials by hand — is a collapsed fallback so the
    // scan path stays uncluttered. If the connect URI failed to build, open the
    // manual credentials by default so the user can still finish by hand.
    return (
      <>
        {connectUri !== null && (
          <section className="flex flex-col items-center gap-4 rounded-2xl border border-border/60 bg-card p-6 shadow-sm">
            <div className="rounded-2xl bg-white p-4 shadow-lg ring-1 ring-black/5">
              <QRCodeSVG
                value={connectUri}
                size={200}
                aria-label={t('devices.mobileSync.credential.pair.qrAlt')}
              />
            </div>
            <p className="max-w-[34ch] text-center text-[15px] font-medium leading-snug text-balance text-foreground">
              {t('devices.mobileSync.deviceDialog.scanInstruction')}
            </p>
            <div className="flex w-full flex-col items-center gap-2 pt-1">
              <span className="text-[10px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                {t('devices.mobileSync.deviceDialog.sections.serverAddress')}
              </span>
              {addressChip}
              <p className="flex items-center gap-1.5 text-[11.5px] text-muted-foreground">
                <Wifi className="size-3.5 text-info" />
                {t('devices.mobileSync.deviceDialog.wifiSameNetwork')}
              </p>
            </div>
          </section>
        )}

        {installQr !== null && <MobileSyncInstallHelper installQrCodePngBase64={installQr} />}

        <ManualCredentials
          username={credentialResult.username}
          password={credentialResult.password ?? ''}
          warning={
            credentialFromRename
              ? t('devices.mobileSync.edit.result.warningReissued')
              : t('devices.mobileSync.edit.result.warning')
          }
          defaultOpen={connectUri === null}
          passwordVisible={passwordVisible}
          setPasswordVisible={setPasswordVisible}
          backupCopied={backupCopied}
          onBackup={onBackup}
        />
      </>
    )
  }

  // ── steady state: facts first, then address + honest re-pair CTA ──
  return (
    <>
      <DeviceDetails device={device} />

      <Section title={t('devices.mobileSync.deviceDialog.sections.serverAddress')}>
        {addressChip}
      </Section>

      <Section title={t('devices.mobileSync.deviceDialog.sections.pairing')}>
        <div className="flex items-center gap-4 rounded-xl border border-border/60 bg-muted/40 p-4">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-xl border border-border/60 bg-card text-muted-foreground">
            <QrCode className="size-5" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-foreground">
              {t('devices.mobileSync.deviceDialog.repair.title')}
            </p>
            <p className="mt-0.5 text-[11.5px] leading-snug text-muted-foreground">
              {t('devices.mobileSync.deviceDialog.repair.description')}
            </p>
          </div>
          <Button variant="outline" size="sm" className="shrink-0" onClick={onRepair}>
            {t('devices.mobileSync.deviceDialog.repair.action')}
          </Button>
        </div>
      </Section>
    </>
  )
}

// ────────────────────────────────────────────────────────────────
// Manual credentials — collapsed fallback for the scan path. Holds the
// one-time password, so the trigger stays visible ("password shown once") and
// the expanded body leads with the loss warning + a one-click backup.
// ────────────────────────────────────────────────────────────────

interface ManualCredentialsProps {
  username: string
  password: string
  warning: string
  defaultOpen: boolean
  passwordVisible: boolean
  setPasswordVisible: (v: boolean) => void
  backupCopied: boolean
  onBackup: (e: React.MouseEvent) => void
}

const ManualCredentials: React.FC<ManualCredentialsProps> = ({
  username,
  password,
  warning,
  defaultOpen,
  passwordVisible,
  setPasswordVisible,
  backupCopied,
  onBackup,
}) => {
  const { t } = useTranslation()
  return (
    <Collapsible
      defaultOpen={defaultOpen}
      className="rounded-xl border border-warning/40 bg-warning/5"
    >
      <CollapsibleTrigger
        render={
          <button
            type="button"
            className="group flex w-full items-center gap-2.5 px-3.5 py-3 text-left"
          />
        }
      >
        <KeyRound className="size-4 shrink-0 text-warning" />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-warning">
          {t('devices.mobileSync.deviceDialog.manualEntry.title')}
        </span>
        <span className="hidden shrink-0 text-[11px] text-muted-foreground sm:inline">
          {t('devices.mobileSync.deviceDialog.manualEntry.oneTimeHint')}
        </span>
        <ChevronRight className="size-3.5 shrink-0 text-muted-foreground transition-transform group-data-[state=open]:rotate-90" />
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 border-t border-warning/30 px-3.5 pt-3 pb-3.5">
        <p className="flex items-start gap-2 text-xs text-warning">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          <span>{warning}</span>
        </p>

        <CredentialRow label={t('devices.mobileSync.credential.username.label')} value={username} />
        <CredentialRow
          label={t('devices.mobileSync.credential.password.label')}
          value={password}
          secret={!passwordVisible}
          extra={
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label={
                passwordVisible
                  ? t('devices.mobileSync.credential.password.hide')
                  : t('devices.mobileSync.credential.password.show')
              }
              title={
                passwordVisible
                  ? t('devices.mobileSync.credential.password.hide')
                  : t('devices.mobileSync.credential.password.show')
              }
              onClick={() => setPasswordVisible(!passwordVisible)}
            >
              {passwordVisible ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
            </Button>
          }
        />

        <div className="flex justify-end">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 border-warning/40 text-warning hover:bg-warning/10 hover:text-warning"
            onClick={onBackup}
            aria-label={t('devices.mobileSync.credential.credentials.backup')}
          >
            {backupCopied ? (
              <>
                <Check className="size-3.5" />
                {t('devices.mobileSync.credential.credentials.backupCopied')}
              </>
            ) : (
              <>
                <Copy className="size-3.5" />
                {t('devices.mobileSync.credential.credentials.backup')}
              </>
            )}
          </Button>
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}

const DeviceDetails: React.FC<{ device: MobileDeviceView }> = ({ device }) => {
  const { t } = useTranslation()
  const createdAt = formatAbsoluteDateTime(device.createdAtMs)
  const lastSeen =
    device.lastSeenAtMs != null
      ? formatAbsoluteDateTime(device.lastSeenAtMs)
      : t('devices.mobileSync.list.lastSeen.never')

  return (
    <Section title={t('devices.mobileSync.deviceDialog.sections.info')}>
      <div className="flex flex-col border-y border-border/50">
        <PanelFactRow label={t('devices.mobileSync.deviceDialog.fields.createdAt')}>
          <span className="text-xs font-medium">{createdAt}</span>
        </PanelFactRow>
        <PanelFactRow label={t('devices.mobileSync.deviceDialog.fields.lastSeen')}>
          <span className="text-xs font-medium">{lastSeen}</span>
        </PanelFactRow>
        {device.lastSeenIp && (
          <PanelFactRow label={t('devices.mobileSync.deviceDialog.fields.lastSeenIp')}>
            <span className="truncate font-mono text-xs font-medium">{device.lastSeenIp}</span>
          </PanelFactRow>
        )}
        {device.reportedName && (
          <PanelFactRow label={t('devices.mobileSync.deviceDialog.fields.reportedName')}>
            <span className="truncate text-xs font-medium">{device.reportedName}</span>
          </PanelFactRow>
        )}
        {device.reportedOs && (
          <PanelFactRow label={t('devices.mobileSync.deviceDialog.fields.reportedOs')}>
            <span className="truncate text-xs font-medium">{device.reportedOs}</span>
          </PanelFactRow>
        )}
      </div>
    </Section>
  )
}

interface EditViewProps {
  labelInput: string
  usernameInput: string
  baseUsername: string
  passwordInput: string
  autoGeneratePassword: boolean
  submitting: boolean
  fieldErrors: FieldErrors
  formError: string | null
  onLabelChange: (value: string) => void
  onUsernameChange: (value: string) => void
  onPasswordChange: (value: string) => void
  onToggleAutoPassword: () => void
}

const EditView: React.FC<EditViewProps> = ({
  labelInput,
  usernameInput,
  baseUsername,
  passwordInput,
  autoGeneratePassword,
  submitting,
  fieldErrors,
  formError,
  onLabelChange,
  onUsernameChange,
  onPasswordChange,
  onToggleAutoPassword,
}) => {
  const { t } = useTranslation()
  // A username change mints a new password server-side and forces re-pairing.
  const usernameRenamed = usernameInput.trim() !== baseUsername.trim()
  return (
    <div className="space-y-5">
      <div className="space-y-1.5">
        <Label htmlFor="mobile-device-edit-label">{t('devices.mobileSync.edit.label.label')}</Label>
        <Input
          id="mobile-device-edit-label"
          autoFocus
          value={labelInput}
          onChange={e => onLabelChange(e.target.value)}
          placeholder={t('devices.mobileSync.edit.label.placeholder')}
          disabled={submitting}
          maxLength={64}
          aria-invalid={fieldErrors.label !== undefined || undefined}
          aria-describedby={fieldErrors.label ? 'mobile-device-edit-label-error' : undefined}
        />
        {fieldErrors.label !== undefined && (
          <p id="mobile-device-edit-label-error" role="alert" className="text-xs text-destructive">
            {fieldErrors.label}
          </p>
        )}
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="mobile-device-edit-username">
          {t('devices.mobileSync.edit.username.label')}
        </Label>
        <Input
          id="mobile-device-edit-username"
          value={usernameInput}
          onChange={e => onUsernameChange(e.target.value)}
          placeholder={t('devices.mobileSync.edit.username.placeholder')}
          disabled={submitting}
          autoComplete="off"
          aria-invalid={fieldErrors.username !== undefined || undefined}
          aria-describedby={
            fieldErrors.username
              ? 'mobile-device-edit-username-error'
              : 'mobile-device-edit-username-help'
          }
        />
        {fieldErrors.username !== undefined ? (
          <p
            id="mobile-device-edit-username-error"
            role="alert"
            className="text-xs text-destructive"
          >
            {fieldErrors.username}
          </p>
        ) : (
          <p id="mobile-device-edit-username-help" className="text-xs text-muted-foreground/80">
            {t('devices.mobileSync.edit.username.help')}
          </p>
        )}
        {usernameRenamed && (
          <p className="flex items-start gap-1.5 text-xs text-warning" role="status">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
            <span>{t('devices.mobileSync.edit.username.renameNotice')}</span>
          </p>
        )}
      </div>

      <div className="space-y-1.5">
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="mobile-device-edit-password">
            {t('devices.mobileSync.edit.password.label')}
          </Label>
          <Button
            type="button"
            variant={autoGeneratePassword ? 'secondary' : 'outline'}
            size="sm"
            onClick={onToggleAutoPassword}
            disabled={submitting}
          >
            <Wand2 className="size-3.5" />
            {t('devices.mobileSync.edit.password.regenerate')}
          </Button>
        </div>
        <Input
          id="mobile-device-edit-password"
          type="password"
          value={passwordInput}
          onChange={e => onPasswordChange(e.target.value)}
          placeholder={
            autoGeneratePassword
              ? t('devices.mobileSync.edit.password.autoPlaceholder')
              : t('devices.mobileSync.edit.password.placeholder')
          }
          disabled={submitting || autoGeneratePassword}
          autoComplete="new-password"
          aria-invalid={fieldErrors.password !== undefined || undefined}
          aria-describedby={
            fieldErrors.password
              ? 'mobile-device-edit-password-error'
              : 'mobile-device-edit-password-help'
          }
        />
        {fieldErrors.password !== undefined ? (
          <p
            id="mobile-device-edit-password-error"
            role="alert"
            className="text-xs text-destructive"
          >
            {fieldErrors.password}
          </p>
        ) : (
          <p id="mobile-device-edit-password-help" className="text-xs text-muted-foreground/80">
            {autoGeneratePassword
              ? t('devices.mobileSync.edit.password.autoHelp')
              : t('devices.mobileSync.edit.password.help')}
          </p>
        )}
      </div>

      {formError !== null && (
        <div
          role="alert"
          className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {formError}
        </div>
      )}
    </div>
  )
}

const Section: React.FC<{
  title: string
  children: React.ReactNode
}> = ({ title, children }) => (
  <section className="space-y-2">
    <h5 className="text-[10px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
      {title}
    </h5>
    <div className="space-y-2">{children}</div>
  </section>
)

const CredentialRow: React.FC<{
  label: string
  value: string
  secret?: boolean
  extra?: React.ReactNode
}> = ({ label, value, secret, extra }) => {
  const display = secret ? value.replace(/./g, '•') : value
  return (
    <div className="flex items-center gap-2">
      <Label className="w-16 shrink-0 text-xs text-muted-foreground">{label}</Label>
      <div className="flex min-w-0 flex-1 items-center gap-1 rounded-md border border-border/60 bg-card px-2 py-1">
        <span
          className={cn('min-w-0 flex-1 truncate font-mono text-sm', secret && 'tracking-widest')}
        >
          {display}
        </span>
        {extra}
        <InlineCopyButton value={value} />
      </div>
    </div>
  )
}

const InlineCopyButton: React.FC<{ value: string }> = ({ value }) => {
  const { t } = useTranslation()
  const { copied, copy } = useCopyToClipboard()
  const label = copied
    ? t('devices.mobileSync.credential.copied')
    : t('devices.mobileSync.credential.copy')
  return (
    <Button
      type="button"
      size="icon-sm"
      variant="ghost"
      aria-label={label}
      title={label}
      onClick={() => void copy(value)}
    >
      {copied ? <Check className="size-3.5 text-success" /> : <Copy className="size-3.5" />}
    </Button>
  )
}

type EditErrorDispatch =
  | { kind: 'field'; field: FieldErrorKey; message: string }
  | { kind: 'form'; message: string }

function classifyEditError(
  t: ReturnType<typeof useTranslation>['t'],
  err: unknown
): EditErrorDispatch {
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
      case 'DEVICE_NOT_FOUND':
        return { kind: 'form', message: t('devices.mobileSync.errors.deviceNotFound') }
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
      case 'FACADE_UNAVAILABLE':
        return { kind: 'form', message: t('devices.mobileSync.errors.facadeUnavailable') }
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return { kind: 'form', message: t('devices.mobileSync.errors.unknown', { message }) }
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return { kind: 'form', message: t('devices.mobileSync.errors.unknown', { message }) }
}

function formatAbsoluteDateTime(ms: number): string {
  const d = new Date(ms)
  const yyyy = d.getFullYear()
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  const hh = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  return `${yyyy}-${mm}-${dd} ${hh}:${mi}`
}
