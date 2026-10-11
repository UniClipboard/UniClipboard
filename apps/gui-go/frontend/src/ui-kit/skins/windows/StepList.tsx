import { Button, makeStyles, tokens } from '@fluentui/react-components'
import type { StepListProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract; the Win11 setup-guide design belongs to the Windows
// History work.
const useStyles = makeStyles({
  list: {
    margin: 0,
    padding: tokens.spacingHorizontalL,
    listStyle: 'none',
    display: 'flex',
    flexDirection: 'column',
    gap: tokens.spacingVerticalM,
  },
  step: { display: 'flex', flexDirection: 'column', gap: tokens.spacingVerticalXS },
  title: { fontWeight: tokens.fontWeightSemibold },
  text: { color: tokens.colorNeutralForeground3 },
})

export function StepList({ label, steps }: StepListProps) {
  const styles = useStyles()
  return (
    <ol className={styles.list} aria-label={label}>
      {steps.map((step, index) => (
        <li key={step.id} className={styles.step}>
          <span className={styles.title}>{`${index + 1}. ${step.title}`}</span>
          {step.description ? <span className={styles.text}>{step.description}</span> : null}
          {step.status ? <span className={styles.text}>{step.status.text}</span> : null}
          {step.action ? (
            <Button
              appearance={step.action.primary ? 'primary' : 'secondary'}
              onClick={step.action.onClick}
            >
              {step.action.label}
            </Button>
          ) : null}
        </li>
      ))}
    </ol>
  )
}
