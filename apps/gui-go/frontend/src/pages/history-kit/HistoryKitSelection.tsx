import { useTranslation } from 'react-i18next'
import { ActionBar, Button, Text } from '@/ui-kit'
import { HistoryKitSendMenu } from './HistoryKitSendMenu'

interface HistoryKitSelectionProps {
  ids: string[]
  total: number
  allPinned: boolean
  onPinAll: () => void
  onDeleteAll: () => void
  onDeselect: () => void
}

/**
 * The detail pane while several rows are selected (design MC2): the count and the commands that
 * act on all of them. "Copy as one item" and "Copy N Items" from the design are not offered: the
 * Engine restores one entry to the clipboard at a time.
 */
export function HistoryKitSelection({
  ids,
  total,
  allPinned,
  onPinAll,
  onDeleteAll,
  onDeselect,
}: HistoryKitSelectionProps) {
  const { t } = useTranslation()
  return (
    <div className="relative flex min-h-0 flex-1 flex-col" data-testid="clipboard-selection">
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pb-28 pt-1">
        <div className="flex shrink-0 items-baseline gap-2">
          <Text role="title">{t('history.kit.selectedTitle', { count: ids.length })}</Text>
          <Text role="caption" tone="secondary">
            {t('history.kit.selectedOf', { count: ids.length, total })}
          </Text>
          <span className="flex-1" />
          <Button variant="subtle" size="small" onClick={onDeselect}>
            {t('history.kit.deselectAll')}
          </Button>
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
          <Button icon="pin" onClick={onPinAll}>
            {t(allPinned ? 'history.kit.unpinAll' : 'history.kit.pinAll')}
          </Button>
        </div>
      </div>
      <ActionBar hint={t('history.kit.deselectHint')}>
        <HistoryKitSendMenu entryIds={ids} />
        <Button size="large" icon="delete" onClick={onDeleteAll}>
          {t('history.kit.deleteCount', { count: ids.length })}
        </Button>
      </ActionBar>
    </div>
  )
}
