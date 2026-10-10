import { useState } from 'react'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import { Popover, PopoverContent } from '@/components/ui/popover'
import TagColorPicker from './TagColorPicker'

interface HistoryTagCustomColorProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The control the picker hangs from (the row's actions button). */
  anchor: React.RefObject<HTMLElement | null>
  /** The tag's color now. */
  color: HistoryTagColorDto | undefined
  /** Save a new color; called once, when the picker closes. */
  onSave: (color: HistoryTagColorDto) => void
}

/**
 * A Library row's color picker, opened from its "Color › Custom…" menu
 * item. Dragging through the color area only previews; the color is saved
 * once, when the picker closes, instead of on every step of a drag.
 */
function HistoryTagCustomColor({
  open,
  onOpenChange,
  anchor,
  color,
  onSave,
}: HistoryTagCustomColorProps) {
  const [draft, setDraft] = useState<HistoryTagColorDto | null>(null)
  return (
    <Popover
      open={open}
      onOpenChange={next => {
        if (!next && draft && draft !== color) onSave(draft)
        if (!next) setDraft(null)
        onOpenChange(next)
      }}
    >
      <PopoverContent anchor={anchor} align="end" className="w-auto p-3">
        <TagColorPicker value={draft ?? color} onChange={setDraft} />
      </PopoverContent>
    </Popover>
  )
}

export default HistoryTagCustomColor
