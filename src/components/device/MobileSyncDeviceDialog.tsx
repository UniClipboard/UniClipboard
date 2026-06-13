import {
  AlertTriangle,
  Check,
  Copy,
  Eye,
  EyeOff,
  KeyRound,
  Loader2,
  Pencil,
  Smartphone,
  Trash2,
  Wand2,
} from 'lucide-react'
import { QRCodeSVG } from 'qrcode.react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isMobileSyncError,
  listMobileLanInterfaces,
  updateMobileDevice,
  type LanInterfaceView,
  type MobileDeviceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
  type UpdateMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { BaseUrlChip } from '@/components/device/MobileSyncBaseUrlChip'
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
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { buildConnectUri } from '@/lib/mobileSyncConnectUri'
import { cn } from '@/lib/utils'

const log = createLogger('mobile-sync-device-dialog')

type View = 'info' | 'edit'
type FieldErrorKey = 'label' | 'username' | 'password'
type FieldErrors = Partial<Record<FieldErrorKey, string>>

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  device: MobileDeviceView | null
  settings: MobileSyncSettingsView | null
  onRevoke: (device: MobileDeviceView) => void
  onRotated: () => void
}

const MobileSyncDeviceDialog: React.FC<Props> = ({
  open,
  onOpenChange,
  device,
  settings,
  onRevoke,
  onRotated,
}) => {
  const { t } = useTranslation()

  const [view, setView] = useState<View>('info')
  const [credentialResult, setCredentialResult] = useState<UpdateMobileDeviceResult | null>(null)
  const [credentialFromRename, setCredentialFromRename] = useState(false)

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
  const [backupCopied, setBackupCopied] = useState(false)

  useEffect(() => {
    if (open) {
      setView('info')
      setCredentialResult(null)
      setCredentialFromRename(false)
      setLabelInput('')
      setEditBaseUsername('')
      setUsernameInput('')
      setPasswordInput('')
      setAutoGeneratePassword(false)
      setFieldErrors({})
      setFormError(null)
      setSubmitting(false)
      setSelectedHost(null)
      setPasswordVisible(false)
      setBackupCopied(false)
    }
  }, [open, device?.deviceId])

  useEffect(() => {
    if (!open) return
    let cancelled = false
    listMobileLanInterfaces()
      .then(list => {
        if (!cancelled) setLanInterfaces(list)
      })
      .catch(err => log.warn({ err }, 'failed to list LAN interfaces'))
    return () => {
      cancelled = true
    }
  }, [open])

  const visibleLabel = credentialResult?.label ?? device?.label ?? ''
  const visibleUsername = credentialResult?.username ?? device?.username ?? ''

  const port = useMemo(() => {
    if (settings?.lanPort != null) return String(settings.lanPort)
    return '42720'
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

  useEffect(() => {
    if (selectedHost !== null) return
    if (dropdownInterfaces.length === 0) return
    if (preferredHost !== null && dropdownInterfaces.some(i => i.ipv4 === preferredHost)) {
      setSelectedHost(preferredHost)
    } else {
      setSelectedHost(dropdownInterfaces[0].ipv4)
    }
  }, [dropdownInterfaces, preferredHost, selectedHost])

  const effectiveBaseUrl = useMemo(() => {
    const host = selectedHost ?? preferredHost ?? '0.0.0.0'
    return `http://${host}:${port}`
  }, [selectedHost, preferredHost, port])

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
      log.warn({ err }, 'failed to build connect URI in device dialog')
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

  const handleOpenEdit = useCallback(() => {
    if (!device) return
    setLabelInput(visibleLabel)
    setEditBaseUsername(visibleUsername)
    setUsernameInput(visibleUsername)
    setPasswordInput('')
    setAutoGeneratePassword(false)
    setFieldErrors({})
    setFormError(null)
    setView('edit')
  }, [device, visibleLabel, visibleUsername])

  const handleSubmitEdit = useCallback(async () => {
    if (!device) return
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
      setPasswordInput('')
      setAutoGeneratePassword(false)
      setPasswordVisible(false)
      setBackupCopied(false)
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
    device,
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
      try {
        await navigator.clipboard.writeText(text)
        setBackupCopied(true)
        window.setTimeout(() => setBackupCopied(false), 1500)
      } catch {
        toast.error(t('devices.mobileSync.credential.copyFailed'))
      }
    },
    [credentialResult, effectiveBaseUrl, t]
  )

  if (!device) return null

  return (
    <Dialog
      open={open}
      onOpenChange={next => {
        if (!submitting) onOpenChange(next)
      }}
    >
      <DialogContent
        className="flex max-h-[90vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-md"
        onEscapeKeyDown={e => {
          if (submitting) e.preventDefault()
        }}
        onPointerDownOutside={e => {
          if (submitting) e.preventDefault()
        }}
      >
        <DialogHeader className="px-5 pt-5 pb-3">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-info/10 text-info">
              <Smartphone className="h-5 w-5" />
            </div>
            <div className="min-w-0 flex-1">
              <DialogTitle className="truncate text-left">
                {view === 'edit' ? t('devices.mobileSync.edit.title') : visibleLabel}
              </DialogTitle>
              <DialogDescription className="truncate font-mono text-xs text-muted-foreground">
                {visibleUsername}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="flex-1 space-y-5 overflow-y-auto px-5 pb-4">
          {view === 'info' ? (
            <InfoView
              device={device}
              effectiveBaseUrl={effectiveBaseUrl}
              dropdownInterfaces={dropdownInterfaces}
              port={port}
              selectedHost={selectedHost}
              onSelectHost={setSelectedHost}
              connectUri={connectUri}
              credentialResult={credentialResult}
              credentialFromRename={credentialFromRename}
              passwordVisible={passwordVisible}
              setPasswordVisible={setPasswordVisible}
              backupCopied={backupCopied}
              onBackup={handleBackup}
            />
          ) : (
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
          )}
        </div>

        <DialogFooter className="m-0 !flex-row !justify-between gap-2">
          {view === 'info' ? (
            <>
              <Button variant="outline" size="sm" onClick={handleOpenEdit}>
                <Pencil className="h-3.5 w-3.5" />
                {t('devices.mobileSync.edit.button')}
              </Button>
              <div className="flex gap-2">
                <Button variant="destructive" size="sm" onClick={() => onRevoke(device)}>
                  <Trash2 className="h-3.5 w-3.5" />
                  {t('devices.mobileSync.revoke.confirm')}
                </Button>
                <Button size="sm" onClick={() => onOpenChange(false)}>
                  {t('devices.mobileSync.credential.close')}
                </Button>
              </div>
            </>
          ) : (
            <>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setView('info')}
                disabled={submitting}
              >
                {t('devices.mobileSync.edit.cancel')}
              </Button>
              <Button
                size="sm"
                onClick={handleSubmitEdit}
                disabled={submitting || labelInput.trim() === '' || usernameInput.trim() === ''}
              >
                {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
                {submitting
                  ? t('devices.mobileSync.edit.saving')
                  : t('devices.mobileSync.edit.save')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default MobileSyncDeviceDialog

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
  passwordVisible: boolean
  setPasswordVisible: (v: boolean) => void
  backupCopied: boolean
  onBackup: (e: React.MouseEvent) => void
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
  passwordVisible,
  setPasswordVisible,
  backupCopied,
  onBackup,
}) => {
  const { t } = useTranslation()
  const createdAt = formatAbsoluteDateTime(device.createdAtMs)
  const lastSeen =
    device.lastSeenAtMs != null
      ? formatAbsoluteDateTime(device.lastSeenAtMs)
      : t('devices.mobileSync.list.lastSeen.never')

  return (
    <>
      <Section title={t('devices.mobileSync.deviceDialog.sections.serverAddress')}>
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border/60 bg-muted/30 p-4">
          <div className="flex w-full flex-col items-center gap-1.5">
            {dropdownInterfaces.length > 1 && (
              <span className="text-xs text-muted-foreground">
                {t('devices.mobileSync.credential.pair.wifiHint')}
              </span>
            )}
            <BaseUrlChip
              baseUrl={effectiveBaseUrl}
              interfaces={dropdownInterfaces}
              port={port}
              selectedHost={selectedHost}
              onSelect={onSelectHost}
            />
          </div>

          {connectUri !== null ? (
            <>
              <div className="rounded-md bg-white p-3">
                <QRCodeSVG
                  value={connectUri}
                  size={208}
                  aria-label={t('devices.mobileSync.credential.pair.qrAlt')}
                />
              </div>
              <p className="text-center text-xs text-muted-foreground">
                {t('devices.mobileSync.credential.pair.connectHint')}
              </p>
            </>
          ) : (
            <QrPlaceholder />
          )}
        </div>
      </Section>

      {credentialResult?.password && (
        <Section title={t('devices.mobileSync.credential.credentials.title')} accent="amber">
          <div className="space-y-3 rounded-md border border-amber-500/40 bg-amber-500/5 p-3">
            <div className="flex items-start gap-2">
              <AlertTriangle className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
              <p className="flex-1 text-xs text-amber-700/90 dark:text-amber-400/90">
                {credentialFromRename
                  ? t('devices.mobileSync.edit.result.warningReissued')
                  : t('devices.mobileSync.edit.result.warning')}
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 shrink-0 border-amber-500/40 text-amber-700 hover:bg-amber-500/10 hover:text-amber-700 dark:text-amber-400 dark:hover:text-amber-400"
                onClick={onBackup}
                aria-label={t('devices.mobileSync.credential.credentials.backup')}
              >
                {backupCopied ? (
                  <>
                    <Check className="h-3.5 w-3.5" />
                    {t('devices.mobileSync.credential.credentials.backupCopied')}
                  </>
                ) : (
                  <>
                    <Copy className="h-3.5 w-3.5" />
                    {t('devices.mobileSync.credential.credentials.backup')}
                  </>
                )}
              </Button>
            </div>

            <CredentialRow
              label={t('devices.mobileSync.credential.username.label')}
              value={credentialResult.username}
            />
            <CredentialRow
              label={t('devices.mobileSync.credential.password.label')}
              value={credentialResult.password}
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
                  {passwordVisible ? (
                    <EyeOff className="h-3.5 w-3.5" />
                  ) : (
                    <Eye className="h-3.5 w-3.5" />
                  )}
                </Button>
              }
            />
          </div>
        </Section>
      )}

      <Section title={t('devices.mobileSync.deviceDialog.sections.info')}>
        <InfoRow label={t('devices.mobileSync.deviceDialog.fields.createdAt')} value={createdAt} />
        <InfoRow label={t('devices.mobileSync.deviceDialog.fields.lastSeen')} value={lastSeen} />
        {device.lastSeenIp && (
          <InfoRow
            label={t('devices.mobileSync.deviceDialog.fields.lastSeenIp')}
            value={device.lastSeenIp}
            mono
          />
        )}
        {device.reportedName && (
          <InfoRow
            label={t('devices.mobileSync.deviceDialog.fields.reportedName')}
            value={device.reportedName}
          />
        )}
        {device.reportedOs && (
          <InfoRow
            label={t('devices.mobileSync.deviceDialog.fields.reportedOs')}
            value={device.reportedOs}
          />
        )}
      </Section>
    </>
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
    <div className="space-y-4">
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
          <p
            className="flex items-start gap-1.5 text-xs text-amber-700/90 dark:text-amber-400/90"
            role="status"
          >
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
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
            <Wand2 className="h-3.5 w-3.5" />
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
          className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {formError}
        </div>
      )}
    </div>
  )
}

const Section: React.FC<{
  title: string
  accent?: 'default' | 'amber'
  children: React.ReactNode
}> = ({ title, accent = 'default', children }) => (
  <section className="space-y-2">
    <h5
      className={cn(
        'px-1 text-[11px] uppercase tracking-wider',
        accent === 'amber' ? 'text-amber-700/80 dark:text-amber-400/80' : 'text-muted-foreground'
      )}
    >
      {title}
    </h5>
    <div className="space-y-2">{children}</div>
  </section>
)

const InfoRow: React.FC<{ label: string; value: string; mono?: boolean }> = ({
  label,
  value,
  mono,
}) => (
  <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/50 px-3 py-2 text-xs">
    <span className="shrink-0 text-muted-foreground">{label}</span>
    <span className={cn('min-w-0 truncate text-foreground', mono && 'font-mono')} title={value}>
      {value}
    </span>
  </div>
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
  const [copied, setCopied] = useState(false)
  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error(t('devices.mobileSync.credential.copyFailed'))
    }
  }, [t, value])
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
      onClick={handleCopy}
    >
      {copied ? (
        <Check className="h-3.5 w-3.5 text-emerald-500" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </Button>
  )
}

const QrPlaceholder: React.FC = () => {
  const { t } = useTranslation()
  return (
    <div className="flex h-[208px] w-[208px] flex-col items-center justify-center gap-3 rounded-md border-2 border-dashed border-border/60 bg-card/50 p-4 text-center">
      <KeyRound className="h-8 w-8 text-muted-foreground/50" />
      <p className="text-xs leading-snug text-muted-foreground">
        {t('devices.mobileSync.deviceDialog.qrPlaceholder')}
      </p>
    </div>
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
