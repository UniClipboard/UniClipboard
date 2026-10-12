import { useState } from 'react'
import { exportStartupLogs } from '@/api/startup-support'

/** Exports startup diagnostics; `done` only when the user actually saved a file. */
export function useDiagnosticsExport() {
  const [exportState, setExportState] = useState<'idle' | 'exporting' | 'done' | 'failed'>('idle')
  const exportDiagnostics = async () => {
    if (exportState === 'exporting') return
    setExportState('exporting')
    try {
      const path = await exportStartupLogs()
      setExportState(path ? 'done' : 'idle')
    } catch {
      setExportState('failed')
    }
  }
  return { exportState, exportDiagnostics }
}
