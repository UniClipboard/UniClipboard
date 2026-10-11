import { useTranslation } from 'react-i18next'
import type { HistoryRowChip } from '@/components/history/list/history-list-kind'
import { Icon, Tag } from '@/ui-kit'
import { HistoryKitTag } from './HistoryKitTag'

/** Pin mark and tag chips at the end of a history row. */
export function HistoryKitRowTrailing({
  pinned,
  chips,
}: {
  pinned: boolean
  chips: HistoryRowChip[]
}) {
  const { t } = useTranslation()
  return (
    <>
      {pinned && <Icon name="pin" tone="highlight" label={t('history.sidebar.pinned')} />}
      {chips.map(chip =>
        chip.kind === 'local' ? (
          <HistoryKitTag key={chip.id} id={chip.id} name={chip.name} />
        ) : (
          <Tag key={chip.tag} name={`#${t(`history.type.${chip.tag}`)}`} />
        )
      )}
    </>
  )
}
