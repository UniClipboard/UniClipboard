import { useEffect, useState } from 'react'
import {
  DEFAULT_UI_SCALE,
  readStoredUiScale,
  setUiScale,
  subscribeUiScaleChanges,
  type UiScaleOption,
  UI_SCALE_OPTIONS,
} from '@/lib/ui-scale'

export function useUiScale() {
  const [scale, setScaleState] = useState(() => readStoredUiScale())

  useEffect(() => {
    setScaleState(readStoredUiScale())
    return subscribeUiScaleChanges(nextScale => {
      setScaleState(nextScale)
    })
  }, [])

  return {
    scale,
    scalePercent: `${Math.round(scale * 100)}%`,
    options: UI_SCALE_OPTIONS,
    setScale: (nextScale: number) => setUiScale(nextScale),
    resetScale: () => setUiScale(DEFAULT_UI_SCALE),
    isDefault: scale === DEFAULT_UI_SCALE,
    isSelected: (option: UiScaleOption) => option.value === scale,
  }
}
