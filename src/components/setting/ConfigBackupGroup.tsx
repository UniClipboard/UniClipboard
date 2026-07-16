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
import { asConfigError, isCancelled, useConfigImport } from '@/hooks/useConfigImport'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'

const log = createLogger('config-backup')

const I18N = 'settings.sections.storage.configBackup'

/** Maps a bundle's recorded source mode to its translation key. */
const SOURCE_MODE_LABEL_KEYS: Record<string, string> = {
  portable: `${I18N}.import.metaSourcePortable`,
  installed: `${I18N}.import.metaSourceInstalled`,
}

export function ConfigBackupGroup() {
  const { t } = useTranslation()

  // ── Export state ─────────────────────────────────────────────────
  // Export takes no password: the daemon seals the bundle with the
  // installation's own key material (opening it later needs the space
  // passphrase), so the button goes straight to the save dialog.
  const [exporting, setExporting] = useState(false)

  // ── Import flow ──────────────────────────────────────────────────
  // The command sequence + daemon error classification live in the shared
  // hook; this surface only renders the dialogs and maps failures to toasts.
  const imp = useConfigImport({
    onError: kind => {
      switch (kind) {
        case 'invalidPassword':
          toast.error(t(`${I18N}.import.invalidPasswordError`))
          return
        case 'incompatible':
          toast.error(t(`${I18N}.import.incompatibleError`))
          return
        default:
          toast.error(t(`${I18N}.import.genericError`))
      }
    },
    onStaged: result => {
      if (result.unlockRequiredAfterApply) {
        toast.message(t(`${I18N}.import.restartingUnlockHint`))
      }
    },
  })

  // ── Export handler ───────────────────────────────────────────────

  const handleExport = async () => {
    setExporting(true)
    try {
      const result = await commands.exportConfigPackage()
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
      if (isCancelled(error)) return
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

  const sourceModeLabel = (mode: string) => {
    const key = SOURCE_MODE_LABEL_KEYS[mode]
    return key ? t(key) : mode
  }

  return (
    <SettingGroup title={t(`${I18N}.label`)}>
      <SettingRow label={t(`${I18N}.export.label`)} description={t(`${I18N}.export.description`)}>
        <Button variant="outline" size="sm" onClick={handleExport} disabled={exporting}>
          {exporting ? t(`${I18N}.export.exporting`) : t(`${I18N}.export.button`)}
        </Button>
      </SettingRow>

      <SettingRow label={t(`${I18N}.import.label`)} description={t(`${I18N}.import.description`)}>
        <Button variant="outline" size="sm" onClick={imp.pickFile} disabled={imp.busy}>
          {t(`${I18N}.import.button`)}
        </Button>
      </SettingRow>

      {/* ── Import password dialog ── */}
      <Dialog
        open={imp.phase === 'password'}
        onOpenChange={open => {
          if (imp.busy) return
          if (!open) imp.reset()
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
              value={imp.password}
              onChange={e => imp.setPassword(e.target.value)}
              placeholder={t(`${I18N}.import.passwordPlaceholder`)}
              disabled={imp.busy}
              onKeyDown={e => {
                if (e.key === 'Enter' && imp.password && !imp.busy) {
                  void imp.submitPassword()
                }
              }}
            />
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={imp.reset} disabled={imp.busy}>
              {t(`${I18N}.import.cancelButton`)}
            </Button>
            <Button onClick={imp.submitPassword} disabled={imp.busy || !imp.password}>
              {imp.busy ? t(`${I18N}.import.staging`) : t(`${I18N}.import.passwordConfirmButton`)}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Import confirmation (device-move warning) ── */}
      <AlertDialog
        open={imp.phase === 'confirm' || imp.isRestarting}
        onOpenChange={open => {
          // While restarting, the dialog is forced: ignore every close request.
          if (imp.isRestarting) return
          if (!open) imp.reset()
        }}
      >
        <AlertDialogContent
          className="bg-card text-card-foreground"
          onEscapeKeyDown={event => {
            if (imp.isRestarting) event.preventDefault()
          }}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>
              {imp.isRestarting
                ? t(`${I18N}.import.restartingTitle`)
                : t(`${I18N}.import.confirmTitle`)}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {imp.isRestarting
                ? t(`${I18N}.import.restartingDescription`)
                : t(`${I18N}.import.confirmDescription`)}
            </AlertDialogDescription>
          </AlertDialogHeader>

          {imp.isRestarting ? (
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
                {['warningMove', 'warningNoDualOnline', 'warningReplace'].map(warningKey => (
                  <div
                    key={warningKey}
                    className="flex items-start gap-2 text-xs text-foreground/90"
                  >
                    <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-destructive" />
                    <span className="leading-snug">{t(`${I18N}.import.${warningKey}`)}</span>
                  </div>
                ))}
              </div>

              {/* Preview metadata */}
              {imp.preview && (
                <div className="space-y-1.5">
                  <div className="text-xs font-medium text-muted-foreground">
                    {t(`${I18N}.import.metaTitle`)}
                  </div>
                  <dl className="space-y-1 text-xs">
                    <div className="flex justify-between gap-4">
                      <dt className="text-muted-foreground">
                        {t(`${I18N}.import.metaAppVersion`)}
                      </dt>
                      <dd className="tabular-nums">{imp.preview.appVersion}</dd>
                    </div>
                    <div className="flex justify-between gap-4">
                      <dt className="text-muted-foreground">
                        {t(`${I18N}.import.metaSourceMode`)}
                      </dt>
                      <dd>{sourceModeLabel(imp.preview.sourceMode)}</dd>
                    </div>
                    <div className="flex justify-between gap-4">
                      <dt className="text-muted-foreground">
                        {t(`${I18N}.import.metaFingerprint`)}
                      </dt>
                      <dd className="max-w-56 truncate font-mono">
                        {imp.preview.deviceFingerprint}
                      </dd>
                    </div>
                  </dl>
                </div>
              )}

              <AlertDialogFooter>
                <AlertDialogCancel onClick={imp.reset} disabled={imp.busy}>
                  {t(`${I18N}.import.cancelButton`)}
                </AlertDialogCancel>
                <AlertDialogAction
                  onClick={event => {
                    // Keep the dialog open to show the forced restarting state.
                    event.preventDefault()
                    void imp.confirmImport()
                  }}
                  disabled={imp.busy}
                  className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                >
                  {imp.busy ? t(`${I18N}.import.staging`) : t(`${I18N}.import.confirmButton`)}
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
