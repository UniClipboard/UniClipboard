import { Slider as FluentSlider, makeStyles } from '@fluentui/react-components'
import type { SliderProps } from '@/ui-kit/contract'

const useStyles = makeStyles({ slider: { width: '7.5rem', minWidth: '7.5rem' } })

/** WinUI Slider (Fluent Slider: accent track, thumb with the inner accent dot), compact for a command bar. */
export function Slider({ label, value, onValueChange, min, max, step }: SliderProps) {
  const styles = useStyles()
  return (
    <FluentSlider
      className={styles.slider}
      size="small"
      aria-label={label}
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={(_, data) => onValueChange(data.value)}
    />
  )
}
