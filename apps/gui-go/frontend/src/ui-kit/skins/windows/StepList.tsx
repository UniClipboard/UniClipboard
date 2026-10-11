import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import { CheckmarkFilled } from '@fluentui/react-icons'
import type { StepListProps } from '@/ui-kit/contract'
import { Button } from './Button'

// Board WA1: the getting-started steps as cards on the card layer, each with a numbered badge, a
// title, a description, an optional live status (a dot, accent while in progress) and its command.
const useStyles = makeStyles({
  list: {
    margin: 0,
    paddingBlock: '0.5rem',
    paddingInline: '1.25rem',
    listStyle: 'none',
    display: 'flex',
    flexDirection: 'column',
    gap: '0.75rem',
  },
  card: {
    boxSizing: 'border-box',
    display: 'flex',
    gap: '1rem',
    padding: '1.25rem',
    borderRadius: tokens.borderRadiusXLarge,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
  },
  badge: {
    flexShrink: 0,
    width: '2rem',
    height: '2rem',
    borderRadius: tokens.borderRadiusCircular,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontSize: tokens.fontSizeBase300,
    fontWeight: tokens.fontWeightSemibold,
    backgroundColor: 'var(--w11-tag-blue-bg)',
    color: tokens.colorBrandForeground1,
  },
  badgeDone: {
    backgroundColor: tokens.colorBrandBackground,
    color: tokens.colorNeutralForegroundOnBrand,
  },
  body: {
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'flex-start',
    gap: '0.375rem',
  },
  title: {
    margin: 0,
    fontSize: tokens.fontSizeBase400,
    lineHeight: tokens.lineHeightBase400,
    fontWeight: tokens.fontWeightSemibold,
  },
  text: {
    margin: 0,
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    color: tokens.colorNeutralForeground2,
  },
  status: {
    display: 'flex',
    alignItems: 'center',
    gap: '0.5rem',
    fontSize: tokens.fontSizeBase300,
    color: tokens.colorNeutralForeground2,
  },
  dot: {
    width: '0.5rem',
    height: '0.5rem',
    borderRadius: tokens.borderRadiusCircular,
    backgroundColor: 'var(--w11-status-neutral)',
  },
  dotActive: { backgroundColor: tokens.colorBrandBackground },
  action: { marginTop: '0.25rem' },
})

export function StepList({ label, steps }: StepListProps) {
  const styles = useStyles()
  return (
    <ol className={styles.list} aria-label={label}>
      {steps.map((step, index) => (
        <li key={step.id} className={styles.card}>
          <span
            className={mergeClasses(styles.badge, step.done && styles.badgeDone)}
            aria-hidden="true"
          >
            {step.done ? <CheckmarkFilled /> : index + 1}
          </span>
          <div className={styles.body}>
            <h3 className={styles.title}>{step.title}</h3>
            {step.description ? <p className={styles.text}>{step.description}</p> : null}
            {step.status ? (
              <span className={styles.status} role="status">
                <span
                  className={mergeClasses(styles.dot, step.status.active && styles.dotActive)}
                />
                {step.status.text}
              </span>
            ) : null}
            {step.action ? (
              <span className={styles.action}>
                <Button
                  variant={step.action.primary ? 'primary' : 'standard'}
                  onClick={() => step.action?.onClick()}
                >
                  {step.action.label}
                </Button>
              </span>
            ) : null}
          </div>
        </li>
      ))}
    </ol>
  )
}
