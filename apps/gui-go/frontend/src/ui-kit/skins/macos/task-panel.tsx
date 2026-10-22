import type { TaskPanelProps } from '@/ui-kit/contract'
import { Button } from './controls'
import { Text } from './pane'

export function TaskPanel({ title, description, stage, children, back, actions }: TaskPanelProps) {
  return (
    <section className="mac-task">
      <header className="mac-task-header">
        {stage && (
          <Text role="caption" tone="secondary">
            {stage}
          </Text>
        )}
        <Text role="title" emphasized>
          {title}
        </Text>
        {description && <Text tone="secondary">{description}</Text>}
      </header>
      <div className="mac-task-content">{children}</div>
      <footer className="mac-task-actions">
        {back && (
          <Button disabled={back.disabled} onClick={back.onClick}>
            {back.label}
          </Button>
        )}
        <div className="mac-task-primary">{actions}</div>
      </footer>
    </section>
  )
}
