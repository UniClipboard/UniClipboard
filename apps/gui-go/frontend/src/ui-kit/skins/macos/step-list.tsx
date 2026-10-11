import type { StepListProps } from '@/ui-kit/contract'
import { Icon } from './icons'
import { PushButton } from './parts'

/** Getting-started cards with numbered badges (design MA1). */
export function StepList({ label, steps }: StepListProps) {
  return (
    <ol className="mac-steps" aria-label={label}>
      {steps.map((step, index) => (
        <li key={step.id} className="mac-step" data-done={step.done ? '' : undefined}>
          <span className="mac-step-number" aria-hidden="true">
            {step.done ? <Icon name="check" size={12} strokeWidth={2.6} /> : index + 1}
          </span>
          <div className="mac-step-body">
            <span className="mac-step-title">{step.title}</span>
            {step.description && <span className="mac-step-text">{step.description}</span>}
            {step.status && (
              <span className="mac-step-status" data-active={step.status.active ? '' : undefined}>
                {step.status.text}
              </span>
            )}
            {step.action && (
              <PushButton
                tint={step.action.primary ? 'primary' : 'none'}
                onClick={step.action.onClick}
              >
                {step.action.label}
              </PushButton>
            )}
          </div>
        </li>
      ))}
    </ol>
  )
}
