import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { PreviewFrameProps } from '@/ui-kit/contract'

// Board C4 "代码卡片": code on the dark code layer with a caption bar (what it is, and the copy shortcut at the
// end); text on the card layer; an image on a checkerboard stage with its controls in the corners.
const useStyles = makeStyles({
  frame: {
    position: 'relative',
    flexShrink: 0,
    height: 'clamp(10rem, 40vh, 28rem)',
    display: 'flex',
    flexDirection: 'column',
    minHeight: '10rem',
    borderRadius: tokens.borderRadiusXLarge,
    overflow: 'hidden',
  },
  plain: {
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
  },
  code: { backgroundColor: 'var(--w11-code-bg)', color: '#D4D4D4' },
  image: {
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    backgroundColor: 'var(--w11-card-solid)',
    // Transparency checkerboard of an image viewer.
    backgroundImage:
      'linear-gradient(45deg, var(--w11-checker) 25%, transparent 25%), linear-gradient(-45deg, var(--w11-checker) 25%, transparent 25%), linear-gradient(45deg, transparent 75%, var(--w11-checker) 75%), linear-gradient(-45deg, transparent 75%, var(--w11-checker) 75%)',
    backgroundSize: '16px 16px',
    backgroundPosition: '0 0, 0 8px, 8px -8px, -8px 0',
  },
  caption: {
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
    minHeight: '2.25rem',
    paddingInline: '1rem',
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
    borderBottom: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
  },
  codeCaption: {
    color: 'rgba(255, 255, 255, 0.6)',
    backgroundColor: 'rgba(255, 255, 255, 0.04)',
    borderBottomColor: 'rgba(255, 255, 255, 0.06)',
  },
  trailing: { marginInlineStart: 'auto', fontFamily: tokens.fontFamilyMonospace },
  body: { position: 'relative', flexGrow: 1, minHeight: 0, overflow: 'auto' },
  topEnd: { position: 'absolute', top: '0.5rem', insetInlineEnd: '0.5rem', zIndex: 1 },
  bottomStart: { position: 'absolute', bottom: '0.5rem', insetInlineStart: '0.5rem', zIndex: 1 },
})

export function PreviewFrame({
  kind,
  caption,
  captionTrailing,
  topEnd,
  bottomStart,
  children,
}: PreviewFrameProps) {
  const styles = useStyles()
  return (
    <div className={mergeClasses(styles.frame, styles[kind])} data-preview={kind}>
      {caption ? (
        <div className={mergeClasses(styles.caption, kind === 'code' && styles.codeCaption)}>
          <span>{caption}</span>
          {captionTrailing ? (
            <span className={styles.trailing} aria-hidden="true">
              {captionTrailing}
            </span>
          ) : null}
        </div>
      ) : null}
      <div className={styles.body}>{children}</div>
      {topEnd ? <div className={styles.topEnd}>{topEnd}</div> : null}
      {bottomStart ? <div className={styles.bottomStart}>{bottomStart}</div> : null}
    </div>
  )
}
