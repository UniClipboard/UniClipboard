import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { KindBadgeProps } from '@/ui-kit/contract'

const tint = (color: 'blue' | 'orange' | 'neutral') => ({
  backgroundColor: `var(--w11-tag-${color}-bg)`,
  color: `var(--w11-tag-${color}-ink)`,
})

// Type marks in the tag palette; code is a terminal mark in both schemes.
const useStyles = makeStyles({
  tile: {
    boxSizing: 'border-box',
    width: '2rem',
    height: '2rem',
    flexShrink: 0,
    borderRadius: tokens.borderRadiusMedium,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    overflow: 'hidden',
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase100,
    lineHeight: tokens.lineHeightBase100,
    fontWeight: tokens.fontWeightBold,
  },
  pill: {
    boxSizing: 'border-box',
    height: '1.25rem',
    paddingInline: '0.5rem',
    borderRadius: tokens.borderRadiusCircular,
    display: 'inline-flex',
    alignItems: 'center',
    alignSelf: 'flex-start',
    flexShrink: 0,
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    fontWeight: tokens.fontWeightSemibold,
  },
  text: tint('neutral'),
  code: { backgroundColor: '#1F1F1F', color: '#7EE787' },
  link: tint('blue'),
  file: tint('orange'),
  image: tint('orange'),
  thumb: {
    backgroundColor: tokens.colorNeutralBackground3,
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
  },
  thumbImage: { width: '100%', height: '100%', objectFit: 'cover' },
})

/** The type mark of an entry: a square tile (image thumbnail for pictures) or a capsule label. */
export function KindBadge({ kind, label, variant, imageSrc }: KindBadgeProps) {
  const styles = useStyles()
  if (variant === 'tile' && kind === 'image') {
    return (
      <span className={mergeClasses(styles.tile, styles.thumb)} role="img" aria-label={label}>
        {imageSrc ? (
          <img className={styles.thumbImage} src={imageSrc} alt="" draggable={false} />
        ) : null}
      </span>
    )
  }
  return (
    <span className={mergeClasses(styles[variant], styles[kind])} data-kind={kind}>
      {label}
    </span>
  )
}
