import { AlertTriangle, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import * as storageApi from '@/api/storage'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
  Label,
} from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { commands } from '@/lib/ipc'
import type { ConfigCommandError, ConfigImportPreview } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'

const log = createLogger('config-backup')

const I18N = 'settings.sections.storage.configBackup'

/**
 * Narrow an unknown thrown value to the typed {@link ConfigCommandError}. The
 * `commands` proxy rethrows the Rust-side discriminated union verbatim, so call
 * sites can branch on `kind` / daemon `code` instead of scraping messages.
 */
function asConfigError(error: unknown): ConfigCommandError | null {
  if (typeof error === 'object' && error !== null && 'kind' in error) {
    return error as ConfigCommandError
  }
  return null
}

function isCancelled(error: unknown): boolean {
  return asConfigError(error)?.kind === 'cancelled'
}

/**
 * Import flow is a small state machine:
 * - `idle`       — nothing open
 * - `password`   — a file was picked; ask for the import password
 * - `confirm`    — preview loaded; show the irreversible device-move warning
 * - `restarting` — forced, non-dismissable state while the daemon + GUI restart
 *   to apply the staged migration on boot
 */
type ImportPhase = 'idle' | 'password' | 'confirm' | 'restarting'

export function ConfigBackupGroup() {
  const { t } = useTranslation()

  // ── Export state ─────────────────────────────────────────────────
  const [exportOpen, setExportOpen] = useState(false)
  const [exportPassword, setExportPassword] = useState('')
  const [exporting, setExporting] = useState(false)

  // ── Import state ─────────────────────────────────────────────────
  const [importPhase, setImportPhase] = useState<ImportPhase>('idle')
  const [importSourcePath, setImportSourcePath] = useState<string | null>(null)
  const [importPassword, setImportPassword] = useState('')
  const [importPreview, setImportPreview] = useState<ConfigImportPreview | null>(null)
  const [importBusy, setImportBusy] = useState(false)

  const isRestarting = importPhase === 'restarting'

  // ── Export handlers ──────────────────────────────────────────────

  const handleExportConfirm = async () => {
    setExporting(true)
    try {
      const result = await commands.exportConfigPackage(exportPassword)
      setExportOpen(false)
      setExportPassword('')
      toast.success(t(`${I18N}.export.success`))
      // Reveal the bundle so the user can find it immediately. A reveal
      // failure is non-fatal — the export already landed at `result.path`.
      try {
        await storageApi.revealPath(result.path)
      } catch (revealError) {
        log.warn({ err: revealError }, 'Failed to reveal exported config bundle')
      }
    } catch (error) {
      // Save dialog cancelled (the command pops it internally) — silent.
      if (isCancelled(error)) {
        setExportOpen(false)
        setExportPassword('')
        return
      }
      const cfg = asConfigError(error)
      if (cfg?.kind === 'daemon' && cfg.code === 'LOCKED') {
        toast.error(t(`${I18N}.export.lockedError`))
      } else {
        log.error({ err: error }, 'Failed to export config')
        toast.error(t(`${I18N}.export.genericError`))
      }
    } finally {
      setExporting(false)
    }
  }

  // ── Import handlers ──────────────────────────────────────────────

  const handlePickAndStart = async () => {
    try {
      const path = await commands.pickConfigBundlePath()
      // Cancelled the open dialog — silent.
      if (path === null) return
      setImportSourcePath(path)
      setImportPassword('')
      setImportPreview(null)
      setImportPhase('password')
    } catch (error) {
      if (isCancelled(error)) return
      log.error({ err: error }, 'Failed to pick config bundle')
      toast.error(t(`${I18N}.import.genericError`))
    }
  }

  /** Map a daemon error `code` to user-facing copy for the import flow. */
  const reportImportError = (error: unknown) => {
    const cfg = asConfigError(error)
    if (cfg?.kind === 'daemon') {
      switch (cfg.code) {
        case 'ALREADY_INITIALIZED':
          toast.error(t(`${I18N}.import.alreadyInitializedError`))
          return
        case 'INVALID_PASSWORD_OR_CORRUPT':
          toast.error(t(`${I18N}.import.invalidPasswordError`))
          return
        case 'INCOMPATIBLE_BUNDLE':
          toast.error(t(`${I18N}.import.incompatibleError`))
          return
        default:
          break
      }
    }
    log.error({ err: error }, 'Failed to import config')
    toast.error(t(`${I18N}.import.genericError`))
  }

  const handlePreview = async () => {
    if (!importSourcePath) return
    setImportBusy(true)
    try {
      const preview = await commands.previewConfigImport(importPassword, importSourcePath)
      setImportPreview(preview)
      setImportPhase('confirm')
    } catch (error) {
      if (isCancelled(error)) return
      reportImportError(error)
    } finally {
      setImportBusy(false)
    }
  }

  const handleConfirmImport = async () => {
    if (!importSourcePath) return
    setImportBusy(true)
    try {
      const result = await commands.importConfigPackage(importPassword, importSourcePath)
      // Staged successfully — switch into the forced restarting state and let
      // the daemon + GUI restart so the migration lands on boot. restartApp()
      // exits this process, so code after it is unreachable on the happy path.
      setImportPhase('restarting')
      setImportPassword('')
      if (result.unlockRequiredAfterApply) {
        toast.message(t(`${I18N}.import.restartingUnlockHint`))
      }
      await commands.restartDaemon()
      await commands.restartApp()
    } catch (error) {
      if (isCancelled(error)) {
        setImportPhase('confirm')
        return
      }
      reportImportError(error)
      // Drop back to the confirm step so the dialog can be dismissed.
      setImportPhase('confirm')
    } finally {
      setImportBusy(false)
    }
  }

  const closeImport = () => {
    if (isRestarting) return
    setImportPhase('idle')
    setImportSourcePath(null)
    setImportPassword('')
    setImportPreview(null)
  }

  const sourceModeLabel = (mode: string) =>
    mode === 'portable'
      ? t(`${I18N}.import.metaSourcePortable`)
      : mode === 'installed'
        ? t(`${I18N}.import.metaSourceInstalled`)
        : mode

  return (
    <SettingGroup title={t(`${I18N}.label`)}>
      <SettingRow label={t(`${I18N}.export.label`)} description={t(`${I18N}.export.description`)}>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            setExportPassword('')
            setExportOpen(true)
          }}
          disabled={exporting}
        >
          {exporting ? t(`${I18N}.export.exporting`) : t(`${I18N}.export.button`)}
        </Button>
      </SettingRow>

      <SettingRow label={t(`${I18N}.import.label`)} description={t(`${I18N}.import.description`)}>
        <Button variant="outline" size="sm" onClick={handlePickAndStart} disabled={importBusy}>
          {t(`${I18N}.import.button`)}
        </Button>
      </SettingRow>

      {/* ── Export password dialog ── */}
      <Dialog
        open={exportOpen}
        onOpenChange={open => {
          if (exporting) return
          setExportOpen(open)
          if (!open) setExportPassword('')
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t(`${I18N}.export.passwordTitle`)}</DialogTitle>
            <DialogDescription>{t(`${I18N}.export.passwordDescription`)}</DialogDescription>
          </DialogHeader>
          <div className="space-y-1.5">
            <Label htmlFor="config-export-password">{t(`${I18N}.export.passwordLabel`)}</Label>
            <Input
              id="config-export-password"
              type="password"
              value={exportPassword}
              onChange={e => setExportPassword(e.target.value)}
              placeholder={t(`${I18N}.export.passwordPlaceholder`)}
              disabled={exporting}
              onKeyDown={e => {
                if (e.key === 'Enter' && exportPassword && !exporting) {
                  void handleExportConfirm()
                }
              }}
            />
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => {
                setExportOpen(false)
                setExportPassword('')
              }}
              disabled={exporting}
            >
              {t(`${I18N}.import.cancelButton`)}
            </Button>
            <Button onClick={handleExportConfirm} disabled={exporting || !exportPassword}>
              {exporting ? t(`${I18N}.export.exporting`) : t(`${I18N}.export.confirmButton`)}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Import password dialog ── */}
      <Dialog
        open={importPhase === 'password'}
        onOpenChange={open => {
          if (importBusy) return
          if (!open) closeImport()
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t(`${I18N}.import.passwordTitle`)}</DialogTitle>
            <DialogDescription>{t(`${I18N}.import.passwordDescription`)}</DialogDescription>
          </DialogHeader>
          <div className="space-y-1.5">
            <Label htmlFor="config-import-password">{t(`${I18N}.import.passwordLabel`)}</Label>
            <Input
              id="config-import-password"
              type="password"
              value={importPassword}
              onChange={e => setImportPassword(e.target.value)}
              placeholder={t(`${I18N}.import.passwordPlaceholder`)}
              disabled={importBusy}
              onKeyDown={e => {
                if (e.key === 'Enter' && importPassword && !importBusy) {
                  void handlePreview()
                }
              }}
            />
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={closeImport} disabled={importBusy}>
              {t(`${I18N}.import.cancelButton`)}
            </Button>
            <Button onClick={handlePreview} disabled={importBusy || !importPassword}>
              {importBusy ? t(`${I18N}.import.staging`) : t(`${I18N}.import.passwordConfirmButton`)}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Import confirmation (device-move warning) ── */}
      <AlertDialog
        open={importPhase === 'confirm' || isRestarting}
        onOpenChange={open => {
          // While restarting, the dialog is forced: ignore every close request.
          if (isRestarting) return
          if (!open) closeImport()
        }}
      >
        <AlertDialogContent
          className="bg-card text-card-foreground"
          onEscapeKeyDown={event => {
            if (isRestarting) event.preventDefault()
          }}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>
              {isRestarting
                ? t(`${I18N}.import.restartingTitle`)
                : t(`${I18N}.import.confirmTitle`)}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {isRestarting
                ? t(`${I18N}.import.restartingDescription`)
                : t(`${I18N}.import.confirmDescription`)}
            </AlertDialogDescription>
          </AlertDialogHeader>

          {isRestarting ? (
            <AlertDialogFooter>
              <div className="flex w-full items-center justify-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                {t(`${I18N}.import.restartingTitle`)}
              </div>
            </AlertDialogFooter>
          ) : (
            <>
              {/* Irreversible device-move warnings */}
              <div className="space-y-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3">
                {[
                  t(`${I18N}.import.warningMove`),
                  t(`${I18N}.import.warningNoDualOnline`),
                  t(`${I18N}.import.warningReplace`),
                ].map(text => (
                  <div key={text} className="flex items-start gap-2 text-xs text-foreground/90">
                    <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-destructive" />
                    <span className="leading-snug">{text}</span>
                  </div>
                ))}
              </div>

              {/* Preview metadata */}
              {importPreview && (
                <div className="space-y-1.5">
                  <div className="text-xs font-medium text-muted-foreground">
                    {t(`${I18N}.import.metaTitle`)}
                  </div>
                  <dl className="space-y-1 text-xs">
                    <div className="flex justify-between gap-4">
                      <dt className="text-muted-foreground">
                        {t(`${I18N}.import.metaAppVersion`)}
                      </dt>
                      <dd className="tabular-nums">{importPreview.appVersion}</dd>
                    </div>
                    <div className="flex justify-between gap-4">
                      <dt className="text-muted-foreground">
                        {t(`${I18N}.import.metaSourceMode`)}
                      </dt>
                      <dd>{sourceModeLabel(importPreview.sourceMode)}</dd>
                    </div>
                    <div className="flex justify-between gap-4">
                      <dt className="text-muted-foreground">
                        {t(`${I18N}.import.metaFingerprint`)}
                      </dt>
                      <dd className="max-w-56 truncate font-mono">
                        {importPreview.deviceFingerprint}
                      </dd>
                    </div>
                  </dl>
                </div>
              )}

              <AlertDialogFooter>
                <AlertDialogCancel onClick={closeImport} disabled={importBusy}>
                  {t(`${I18N}.import.cancelButton`)}
                </AlertDialogCancel>
                <AlertDialogAction
                  onClick={event => {
                    // Keep the dialog open to show the forced restarting state.
                    event.preventDefault()
                    void handleConfirmImport()
                  }}
                  disabled={importBusy}
                  className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                >
                  {importBusy ? t(`${I18N}.import.staging`) : t(`${I18N}.import.confirmButton`)}
                </AlertDialogAction>
              </AlertDialogFooter>
            </>
          )}
        </AlertDialogContent>
      </AlertDialog>
    </SettingGroup>
  )
}

export default ConfigBackupGroup
