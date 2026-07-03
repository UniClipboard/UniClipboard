import { useTranslation } from 'react-i18next'
import type { ClipboardEntryTag } from '@/lib/clipboard-entry'
import { TAG_STYLE } from './history-card-utils'

interface HistoryCardTagsProps {
  tags?: ClipboardEntryTag[]
}

// Renders the content tags on the card's last line, kept separate from the
// header so the type label stays uncluttered.
function HistoryCardTags({ tags }: HistoryCardTagsProps) {
  const { t } = useTranslation()
  if (!tags?.length) return null

  return (
    <div className="pointer-events-none relative z-10 mt-1.5 flex flex-wrap items-center gap-1">
      {tags.map(tag => (
        <span
          key={tag}
          className="rounded border px-1 py-0 text-[9px] font-medium leading-[1.25]"
          style={{
            backgroundColor: TAG_STYLE[tag].background,
            borderColor: TAG_STYLE[tag].border,
            color: TAG_STYLE[tag].text,
          }}
        >
          {t(`history.type.${tag}`, tag)}
        </span>
      ))}
    </div>
  )
}

export default HistoryCardTags
