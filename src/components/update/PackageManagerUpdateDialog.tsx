//! Dialog that routes Linux deb/rpm users to their system package manager.
//!
//! Tauri's Linux updater can only download/install AppImage payloads, so
//! invoking it on a deb/rpm install either fails outright or — worse —
//! drops files outside the dpkg/rpm DB and leaves the system in an
//! inconsistent state. When `UpdateContext.installKind` is `deb` or `rpm`,
//! `AboutSection`'s check-update flow and `Sidebar`'s update indicator both
//! mount this dialog instead of the regular update dialog. It surfaces the
//! exact upgrade command and a copy/release-page action so the user can
//! finish the upgrade out-of-band.

import { openUrl } from '@tauri-apps/plugin-opener'
import { Check, Copy } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { InstallKind, UpdateMetadata } from '@/api/updater'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('package-manager-update-dialog')

const RELEASE_PAGE_URL = 'https://github.com/UniClipboard/UniClipboard/releases/latest'

interface PackageManagerUpdateDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Must be `'deb'` or `'rpm'`. Other values render nothing. */
  installKind: InstallKind
  /** Update metadata for version display; `null` while not checked. */
  updateInfo: UpdateMetadata | null
}

export const PackageManagerUpdateDialog: React.FC<PackageManagerUpdateDialogProps> = ({
  open,
  onOpenChange,
  installKind,
  updateInfo,
}) => {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  if (installKind !== 'deb' && installKind !== 'rpm') return null

  const hintKey =
    installKind === 'deb' ? 'update.packageManager.debHint' : 'update.packageManager.rpmHint'
  const command = t(
    installKind === 'deb' ? 'update.packageManager.debCommand' : 'update.packageManager.rpmCommand'
  )

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(command)
      setCopied(true)
      toast.success(t('update.packageManager.copied'))
      // Brief visual feedback on the button itself; toast handles the
      // longer "did this work?" confirmation.
      setTimeout(() => setCopied(false), 1500)
    } catch (err) {
      log.warn({ err }, 'clipboard.writeText failed')
    }
  }

  const handleOpenReleasePage = () => {
    openUrl(RELEASE_PAGE_URL).catch(err => log.error({ err }, 'Failed to open release page'))
  }

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('update.packageManager.title')}</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="space-y-3">
              {updateInfo && (
                <div className="space-y-1 text-sm">
                  <div className="flex items-center justify-between text-muted-foreground">
                    <span>{t('update.currentVersion')}</span>
                    <span className="text-foreground">{updateInfo.currentVersion}</span>
                  </div>
                  <div className="flex items-center justify-between text-muted-foreground">
                    <span>{t('update.latestVersion')}</span>
                    <span className="text-foreground">{updateInfo.version}</span>
                  </div>
                </div>
              )}
              <p className="text-sm text-muted-foreground">{t(hintKey)}</p>
              <div className="relative rounded-md border border-border/60 bg-muted/40 px-3 py-2 pr-10 font-mono text-xs text-foreground break-all">
                {command}
                <button
                  type="button"
                  aria-label={t('update.packageManager.copyCommand')}
                  onClick={handleCopy}
                  className="absolute right-1.5 top-1.5 inline-flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                >
                  {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
                </button>
              </div>
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel
            onClick={event => {
              event.preventDefault()
              handleOpenReleasePage()
            }}
          >
            {t('update.packageManager.openReleasePage')}
          </AlertDialogCancel>
          <AlertDialogAction>{t('update.packageManager.ok')}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
