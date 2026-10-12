import { Slider as BaseSlider } from '@base-ui/react/slider'
import type { SliderProps } from '@/ui-kit/contract'

/** Scale for the card size in the header bar. */
export function Slider({ label, value, onValueChange, min, max, step }: SliderProps) {
  return (
    <BaseSlider.Root
      className="adw-slider"
      value={value}
      min={min}
      max={max}
      step={step}
      onValueChange={next => onValueChange(Array.isArray(next) ? next[0] : next)}
    >
      <BaseSlider.Control className="adw-slider-control">
        <BaseSlider.Track className="adw-slider-track">
          <BaseSlider.Indicator className="adw-slider-indicator" />
          <BaseSlider.Thumb className="adw-slider-thumb" aria-label={label} />
        </BaseSlider.Track>
      </BaseSlider.Control>
    </BaseSlider.Root>
  )
}
