import type { TaskPanelProps } from '@/ui-kit/contract'
import { Button } from './controls'
import { Text } from './pane'

export function TaskPanel({ title, description, stage, children, back, actions }: TaskPanelProps) {
  return (
    <section className="adw-task">
      <header className="adw-task-header">
        <Text role="title" emphasized>
          {title}
        </Text>
        {description && <Text tone="secondary">{description}</Text>}
        {stage && (
          <Text role="caption" tone="secondary">
            {stage}
          </Text>
        )}
      </header>
      <div className="adw-task-content">{children}</div>
      <footer className="adw-task-actions">
        {back && (
          <Button disabled={back.disabled} onClick={back.onClick}>
            {back.label}
          </Button>
        )}
        <div className="adw-task-primary">{actions}</div>
      </footer>
    </section>
  )
}
