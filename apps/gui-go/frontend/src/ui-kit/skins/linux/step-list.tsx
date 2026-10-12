import type { StepListProps } from '@/ui-kit/contract'
import { Icon } from './icons'
import { PushButton } from './parts'

/** Getting-started cards with numbered badges . */
export function StepList({ label, steps }: StepListProps) {
  return (
    <ol className="adw-steps" aria-label={label}>
      {steps.map((step, index) => (
        <li key={step.id} className="adw-step" data-done={step.done ? '' : undefined}>
          <span className="adw-step-number" aria-hidden="true">
            {step.done ? <Icon name="check" size={14} strokeWidth={2.6} /> : index + 1}
          </span>
          <div className="adw-step-body">
            <span className="adw-step-title">{step.title}</span>
            {step.description && <span className="adw-step-text">{step.description}</span>}
            {step.status && (
              <span className="adw-step-status" data-active={step.status.active ? '' : undefined}>
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
