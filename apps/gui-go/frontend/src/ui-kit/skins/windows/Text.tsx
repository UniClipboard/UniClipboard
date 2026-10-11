import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { TextProps } from '@/ui-kit/contract'

// Roles on the Windows 11 type ramp: title = Subtitle (20/28 semibold), headline = Body Strong,
// body = Body (14/20), callout and caption = Caption (12/16); WinUI has no step between Body and
// Caption, so the two smallest roles share it.
const useStyles = makeStyles({
  base: { minWidth: 0 },
  title: {
    fontSize: tokens.fontSizeBase500,
    lineHeight: tokens.lineHeightBase500,
    fontWeight: tokens.fontWeightSemibold,
  },
  headline: {
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    fontWeight: tokens.fontWeightSemibold,
  },
  body: { fontSize: tokens.fontSizeBase300, lineHeight: tokens.lineHeightBase300 },
  callout: { fontSize: tokens.fontSizeBase200, lineHeight: tokens.lineHeightBase200 },
  caption: { fontSize: tokens.fontSizeBase200, lineHeight: tokens.lineHeightBase200 },
  primary: { color: tokens.colorNeutralForeground1 },
  secondary: { color: tokens.colorNeutralForeground2 },
  tertiary: { color: tokens.colorNeutralForeground3 },
  positive: { color: 'var(--w11-status-positive)' },
  critical: { color: 'var(--w11-status-critical)' },
  emphasized: { fontWeight: tokens.fontWeightSemibold },
  mono: { fontFamily: tokens.fontFamilyMonospace },
  truncate: {
    display: 'block',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
})

export function Text({
  role = 'body',
  tone = 'primary',
  emphasized,
  monospace,
  truncate,
  children,
}: TextProps) {
  const styles = useStyles()
  return (
    <span
      className={mergeClasses(
        styles.base,
        styles[role],
        styles[tone],
        emphasized && styles.emphasized,
        monospace && styles.mono,
        truncate && styles.truncate
      )}
    >
      {children}
    </span>
  )
}
