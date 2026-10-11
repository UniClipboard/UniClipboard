import { Slider as FluentSlider } from '@fluentui/react-components'
import type { SliderProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract; styling belongs to the Windows History work.
export function Slider({ label, value, onValueChange, min, max, step }: SliderProps) {
  return (
    <FluentSlider
      aria-label={label}
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={(_, data) => onValueChange(data.value)}
    />
  )
}
