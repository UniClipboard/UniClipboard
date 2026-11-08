import { useTranslation } from 'react-i18next'
import type { HistoryManagement } from '@/pages/history-kit/useHistoryManagement'
import { Button } from '@/ui-kit'

/** The same management actions in skins that otherwise hide the sidebar completely. */
export function HistoryManagementRail({ management }: { management: HistoryManagement }) {
  const { t } = useTranslation()
  return (
    <nav aria-label={t('history.sidebar.library')} className="flex flex-col items-center gap-2">
      {management.items.map(item => (
        <Button
          key={item.id}
          size="small"
          icon={item.icon}
          label={item.label}
          onClick={() => management.onSelect(item.id)}
        />
      ))}
    </nav>
  )
}
