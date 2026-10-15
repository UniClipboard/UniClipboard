import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { contactAuthor, exportStartupLogs } from '@/api/startup-support'
import { checkForUpdate, openUpdaterWindow } from '@/api/updater'

export type StatusAction = 'export' | 'contact' | 'update'

async function performStatusAction(next: StatusAction) {
  if (next === 'export') return exportStartupLogs()
  if (next === 'contact') {
    await contactAuthor()
    return null
  }
  await openUpdaterWindow()
  await checkForUpdate(null)
  return null
}

/** One support action at a time on the startup failure screen, with its feedback message. */
export function useStatusActions() {
  const { t } = useTranslation()
  const [action, setAction] = useState<StatusAction | null>(null)
  const [feedback, setFeedback] = useState<{ error: boolean; message: string } | null>(null)

  async function runAction(next: StatusAction) {
    if (action) return
    setAction(next)
    setFeedback(null)
    try {
      const exported = await performStatusAction(next)
      if (next === 'export' && exported)
        setFeedback({
          error: false,
          message: t('startupFailure.exported', { path: exported.path }),
        })
      if (next === 'contact')
        setFeedback({ error: false, message: t('startupFailure.contactOpened') })
    } catch {
      setFeedback({ error: true, message: t(`startupFailure.${next}Failed`) })
    } finally {
      setAction(null)
    }
  }

  return { action, feedback, runAction }
}
