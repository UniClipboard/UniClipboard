import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('general-section')

const DEFAULT_ERROR_KEY = 'settings.sections.general.saveError'

/**
 * Shared mutation wrapper for the General settings sections. Each section owns
 * its own `saving` flag (so an in-flight save only disables that section's
 * controls), while `runSave` gives every handler the same try/flag/catch shape:
 * surface failures through the logger and a toast instead of failing silently.
 */
export function useSavingState() {
  const { t } = useTranslation()
  const [saving, setSaving] = useState(false)

  const runSave = async (
    failureLog: string,
    action: () => Promise<void>,
    errorKey: string = DEFAULT_ERROR_KEY
  ) => {
    try {
      setSaving(true)
      await action()
    } catch (error) {
      log.error({ err: error }, failureLog)
      toast.error(t(errorKey))
    } finally {
      setSaving(false)
    }
  }

  return { saving, runSave }
}
