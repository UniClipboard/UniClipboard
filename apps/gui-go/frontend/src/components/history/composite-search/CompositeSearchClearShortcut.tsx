import { useShortcut } from '@/hooks/useShortcut'

interface CompositeSearchClearShortcutProps {
  enabled: boolean
  onClear: () => void
}

function CompositeSearchClearShortcut({ enabled, onClear }: CompositeSearchClearShortcutProps) {
  useShortcut({
    key: 'esc',
    scope: 'clipboard',
    enabled,
    handler: onClear,
    enableOnFormTags: false,
  })

  return null
}

export default CompositeSearchClearShortcut
