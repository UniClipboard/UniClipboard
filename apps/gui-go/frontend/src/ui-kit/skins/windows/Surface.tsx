import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { SurfaceProps } from '@/ui-kit/contract'

// The Win11 boards draw the pane and the content on the Mica base colour. This round applies no
// native Windows backdrop, so both regions are solid layers.
const useStyles = makeStyles({
  sidebar: { backgroundColor: 'var(--w11-base)', minWidth: 0 },
  content: {
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    borderRadius: tokens.borderRadiusXLarge,
    minWidth: 0,
  },
})

export function Surface({ region, children }: SurfaceProps) {
  const styles = useStyles()
  return (
    <div
      className={mergeClasses(styles[region])}
      data-surface={region}
      data-window-material="solid"
    >
      {children}
    </div>
  )
}
