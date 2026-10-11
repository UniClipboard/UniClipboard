import { Slider as BaseSlider } from '@base-ui/react/slider'
import type { SliderProps } from '@/ui-kit/contract'

/** NSSlider (design ME2 toolbar size slider). */
export function Slider({ label, value, onValueChange, min, max, step }: SliderProps) {
  return (
    <BaseSlider.Root
      className="mac-slider"
      value={value}
      min={min}
      max={max}
      step={step}
      onValueChange={next => onValueChange(Array.isArray(next) ? next[0] : next)}
    >
      <BaseSlider.Control className="mac-slider-control">
        <BaseSlider.Track className="mac-slider-track">
          <BaseSlider.Indicator className="mac-slider-indicator" />
          <BaseSlider.Thumb className="mac-slider-thumb" aria-label={label} />
        </BaseSlider.Track>
      </BaseSlider.Control>
    </BaseSlider.Root>
  )
}
