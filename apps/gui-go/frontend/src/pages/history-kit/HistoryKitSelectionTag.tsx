import { useTranslation } from 'react-i18next'
import { useTagColor } from '@/components/history/tags/tag-colors-context'
import { Tag } from '@/ui-kit'
import { kitTagColor } from './history-kit-model'

/** One tag of the selection: striped with "n of m" when only some items carry it. */
export function HistoryKitSelectionTag({
  tagId,
  name,
  on,
  selected,
  onAddToAll,
  onRemoveFromAll,
}: {
  tagId: string
  name: string
  on: number
  selected: number
  onAddToAll: () => void
  onRemoveFromAll: () => void
}) {
  const { t } = useTranslation()
  const color = useTagColor(tagId)
  const partial = on < selected
  return (
    <Tag
      name={name}
      color={kitTagColor(color)}
      partial={partial}
      detail={partial ? t('history.kit.tagOnSome', { on, total: selected }) : undefined}
      action={
        partial
          ? { label: t('history.tags.addToAll', { name, n: selected }), onClick: onAddToAll }
          : undefined
      }
      remove={{ label: t('history.kit.removeFromAll', { name }), onClick: onRemoveFromAll }}
    />
  )
}
