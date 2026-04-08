import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  DEFAULT_UI_SCALE,
  MAX_UI_SCALE,
  MIN_UI_SCALE,
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

  const currentIndex = useMemo(() => UI_SCALE_OPTIONS.findIndex(o => o.value === scale), [scale])

  const canZoomOut = scale > MIN_UI_SCALE
  const canZoomIn = scale < MAX_UI_SCALE

  const zoomIn = useCallback(() => {
    if (currentIndex >= 0 && currentIndex < UI_SCALE_OPTIONS.length - 1) {
      setUiScale(UI_SCALE_OPTIONS[currentIndex + 1].value)
    } else if (currentIndex < 0) {
      const next = UI_SCALE_OPTIONS.find(o => o.value > scale)
      if (next) setUiScale(next.value)
    }
  }, [currentIndex, scale])

  const zoomOut = useCallback(() => {
    if (currentIndex > 0) {
      setUiScale(UI_SCALE_OPTIONS[currentIndex - 1].value)
    } else if (currentIndex < 0) {
      const prev = [...UI_SCALE_OPTIONS].reverse().find(o => o.value < scale)
      if (prev) setUiScale(prev.value)
    }
  }, [currentIndex, scale])

  return {
    scale,
    scalePercent: `${Math.round(scale * 100)}%`,
    options: UI_SCALE_OPTIONS,
    setScale: (nextScale: number) => setUiScale(nextScale),
    resetScale: () => setUiScale(DEFAULT_UI_SCALE),
    isDefault: scale === DEFAULT_UI_SCALE,
    isSelected: (option: UiScaleOption) => option.value === scale,
    zoomIn,
    zoomOut,
    canZoomIn,
    canZoomOut,
  }
}
