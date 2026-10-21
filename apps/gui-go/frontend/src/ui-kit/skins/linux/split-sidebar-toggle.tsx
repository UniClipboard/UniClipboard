import type { Ref } from 'react'
import type { SplitViewProps } from '@/ui-kit/contract'
import { WithHelpTag } from '@/ui-kit/skins/linux/controls'
import { glyph } from '@/ui-kit/skins/linux/glyph'
import { Icon } from '@/ui-kit/skins/linux/icons'

export function SplitSidebarToggle({
  action,
  hidden,
  buttonRef,
}: {
  action: SplitViewProps['sidebarToggle']
  hidden?: boolean
  buttonRef: Ref<HTMLButtonElement>
}) {
  if (!action) return null
  const help = action.shortcutHint ? `${action.label}  ${action.shortcutHint}` : action.label
  return (
    <WithHelpTag text={help}>
      <button
        ref={buttonRef}
        type="button"
        className="adw-btn adw-split-toggle"
        data-size="medium"
        data-tint="borderless"
        data-icon-only=""
        aria-label={action.label}
        aria-expanded={!hidden}
        onClick={action.onClick}
      >
        <Icon name={glyph('sidebar')} size={16} />
      </button>
    </WithHelpTag>
  )
}
