import type { ColorDotProps } from '@/ui-kit/contract'
import { swatch } from './theme'

// Minimal Windows rendering of the contract; styling belongs to the Windows History work.
export function ColorDot({ color, label }: ColorDotProps) {
  return (
    <span
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      style={{
        display: 'inline-block',
        width: '0.5rem',
        height: '0.5rem',
        borderRadius: '50%',
        backgroundColor: color ? swatch(color) : '#8A8A8A',
      }}
    />
  )
}
