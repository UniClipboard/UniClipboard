import { Button as FluentButton, makeStyles } from '@fluentui/react-components'
import { DismissRegular } from '@fluentui/react-icons'
import type { LabelledAction } from '@/ui-kit/contract'

const useStyles = makeStyles({
  button: {
    minWidth: 'auto',
    height: 'auto',
    padding: '0 0.125rem',
    color: 'inherit',
    '& svg': { fontSize: '0.75rem' },
  },
})

/** The remove control of a tag or search token: its own button, named by the caller's label. */
export function RemoveButton({ action }: { action: LabelledAction }) {
  const styles = useStyles()
  return (
    <FluentButton
      appearance="transparent"
      size="small"
      className={styles.button}
      icon={<DismissRegular />}
      aria-label={action.label}
      onClick={() => action.onClick()}
    />
  )
}
