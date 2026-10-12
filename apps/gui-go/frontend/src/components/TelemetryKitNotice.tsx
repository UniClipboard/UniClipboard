import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { useTelemetryNotice, type TelemetryNoticeProps } from '@/hooks/useTelemetryNotice'
import { AlertDialog, UiRoot } from '@/ui-kit'

/**
 * The first-run telemetry notice on the platform UI kit, mounted through `@ui-telemetry-notice`.
 * The consent logic is `useTelemetryNotice`'s; only the presentation differs from `TelemetryNotice`.
 */
export default function TelemetryKitNotice({
  enabled = true,
  onDismiss,
}: TelemetryNoticeProps = {}) {
  const { t } = useTranslation()
  const colorScheme = useAppliedColorScheme()
  const { open, accept, optOut } = useTelemetryNotice({ enabled, onDismiss })

  // The kit alert focuses its confirming action, which here is the acceptance; a privacy consent
  // keeps focus on Disable (the cancel button, first in the actions row) as the shared dialog does,
  // so Enter never consents by accident. The kit picks its initial focus when the popup mounts, so
  // wait for it to settle before moving focus.
  useEffect(() => {
    if (!open) return
    let frame = 0
    let attempts = 0
    const focusDisable = () => {
      const disable = document.querySelector<HTMLElement>('.adw-alert-actions > button')
      if (disable) {
        disable.focus()
        if (document.activeElement === disable && attempts > 2) return
      }
      if (++attempts < 30) frame = requestAnimationFrame(focusDisable)
    }
    frame = requestAnimationFrame(focusDisable)
    return () => cancelAnimationFrame(frame)
  }, [open])

  return (
    // Mounted outside any kit page, so the alert brings its own theme scope; the zero-size fixed
    // wrapper keeps that scope out of the page layout while the alert itself covers the window.
    <div className="fixed z-50 size-0">
      <UiRoot colorScheme={colorScheme}>
        <AlertDialog
          open={open}
          // The notice closes only through a choice: Escape and outside clicks do not dismiss it, and
          // an opt-out that fails to save keeps it open, so `open` follows the consent state alone.
          onOpenChange={() => {}}
          title={t('settings.sections.general.telemetry.notice.title')}
          description={t('settings.sections.general.telemetry.notice.body')}
          cancelLabel={t('settings.sections.general.telemetry.notice.optOut')}
          onCancel={() => void optOut()}
          confirmLabel={t('settings.sections.general.telemetry.notice.accept')}
          onConfirm={accept}
        />
      </UiRoot>
    </div>
  )
}
