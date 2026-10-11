import { useTranslation } from 'react-i18next'
import { useTagColor } from '@/components/history/tags/tag-colors-context'
import { tagLabel } from '@/lib/search-tags'
import { Tag, type LabelledAction } from '@/ui-kit'
import { kitTagColor } from './history-kit-model'

/** A local tag chip in the tag's colour from the daemon's tag layout. */
export function HistoryKitTag({
  id,
  name,
  remove,
}: {
  id: string
  name: string | null | undefined
  remove?: LabelledAction
}) {
  const { t } = useTranslation()
  const color = useTagColor(id)
  return (
    <Tag
      name={`#${tagLabel({ id, name, isBuiltin: false }, t)}`}
      color={kitTagColor(color)}
      remove={remove}
    />
  )
}
