import { useEffect } from 'react'
import { reportMainWindowPresentationReady } from '@/api/main-window'

export function useMainWindowPresentation(ready: boolean) {
  useEffect(() => {
    if (!ready) return
    // Effects run after the selected page is committed. Hidden webviews may not run animation frames.
    void reportMainWindowPresentationReady().catch(error => {
      console.warn('Failed to report main window presentation readiness', error)
    })
  }, [ready])
}
