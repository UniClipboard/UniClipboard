import { useState } from 'react'

type ActionState = 'idle' | 'working' | 'failed' | 'done'

/**
 * Support actions of the startup screen: export diagnostics, open the support page, check for an
 * update. The support and updater modules load on demand because they pull in IPC and diagnostics,
 * which the early startup screen must not wait for.
 */
export function useStartupActions(onExport: () => Promise<void | boolean> | void) {
  const [exportState, setExportState] = useState<ActionState>('idle')
  const [contactState, setContactState] = useState<ActionState>('idle')
  const [updateState, setUpdateState] = useState<'idle' | 'working' | 'failed'>('idle')

  async function exportLogs() {
    if (exportState === 'working') return
    setExportState('working')
    try {
      const exported = await onExport()
      setExportState(exported === false ? 'idle' : 'done')
    } catch {
      setExportState('failed')
    }
  }

  async function openSupport() {
    if (contactState === 'working') return
    setContactState('working')
    try {
      const { contactAuthor } = await import('@/api/startup-support')
      await contactAuthor()
      setContactState('done')
    } catch {
      setContactState('failed')
    }
  }

  async function checkUpdate() {
    if (updateState === 'working') return
    setUpdateState('working')
    try {
      const { checkForUpdate, openUpdaterWindow } = await import('@/api/updater')
      await openUpdaterWindow()
      await checkForUpdate(null)
      setUpdateState('idle')
    } catch {
      setUpdateState('failed')
    }
  }

  return { exportState, contactState, updateState, exportLogs, openSupport, checkUpdate }
}
