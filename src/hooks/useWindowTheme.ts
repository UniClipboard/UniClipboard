import { useEffect, useRef } from 'react'
import { createWindowThemeController } from '@/lib/window-theme'
import type { Settings } from '@/types/setting'

export function useWindowTheme(general: Settings['general'] | undefined): void {
  const controller = useRef<ReturnType<typeof createWindowThemeController> | null>(null)
  useEffect(() => {
    const current = createWindowThemeController(true)
    controller.current = current
    return () => {
      current.dispose()
      controller.current = null
    }
  }, [])
  useEffect(() => {
    if (general) controller.current?.setGeneral(general)
  }, [general])
}
