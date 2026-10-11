import {
  Button as FluentButton,
  makeStyles,
  mergeClasses,
  MessageBar,
  MessageBarActions,
  MessageBarBody,
  MessageBarTitle,
  tokens,
} from '@fluentui/react-components'
import { DismissRegular } from '@fluentui/react-icons'
import type { InlineNoticeProps } from '@/ui-kit/contract'

// WinUI InfoBar colours only the glyph and the background; the border stays the neutral card
// stroke, unlike Fluent web's tinted borders.
const useStyles = makeStyles({
  root: { minHeight: '3rem', border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)` },
  info: { backgroundColor: 'var(--w11-notice-info)' },
  success: { backgroundColor: 'var(--w11-notice-success)' },
  warning: { backgroundColor: 'var(--w11-notice-warning)' },
  error: { backgroundColor: 'var(--w11-notice-error)' },
})

export function InlineNotice({ tone, title, text, action, dismiss }: InlineNoticeProps) {
  const styles = useStyles()
  return (
    <MessageBar
      intent={tone}
      layout="singleline"
      className={mergeClasses(styles.root, styles[tone])}
    >
      <MessageBarBody>
        <MessageBarTitle>{title}</MessageBarTitle>
        {text}
      </MessageBarBody>
      <MessageBarActions
        containerAction={
          dismiss ? (
            <FluentButton
              appearance="transparent"
              aria-label={dismiss.label}
              icon={<DismissRegular />}
              onClick={() => dismiss.onClick()}
            />
          ) : undefined
        }
      >
        {action ? (
          <FluentButton onClick={() => action.onClick()}>{action.label}</FluentButton>
        ) : null}
      </MessageBarActions>
    </MessageBar>
  )
}
