import { Button } from '@/components/ui/button'
import type { InlineNoticeProps } from '@/ui-kit/contract'

/** Untargeted build view; remove with the existing legacy setting parts. */
export function SettingNotice({ tone, title, text, action }: InlineNoticeProps) {
  return (
    <div
      role={tone === 'error' ? 'alert' : 'status'}
      className="flex flex-col gap-2 rounded-lg border border-warning/30 bg-warning/5 p-3 text-ui-body text-foreground"
    >
      <p className="font-medium">{title}</p>
      {text && <p>{text}</p>}
      {action && (
        <Button variant="outline" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  )
}
