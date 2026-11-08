import { useTranslation } from 'react-i18next'
import { useGetReleaseAnnouncementsQuery } from '@/api/announcements'
import { ReleaseAnnouncementsContent } from '@/components/notifications/ReleaseAnnouncementsContent'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import { Button, Sheet } from '@/ui-kit'

export function ReleaseNotificationCenter({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t, i18n } = useTranslation()
  const { isFetching, refetch } = useGetReleaseAnnouncementsQuery(i18n.language, { skip: !open })
  useShortcutLayer({ layer: 'modal', scope: 'modal', enabled: open })
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('notifications.title')}
      footer={
        open ? (
          <Button
            icon="retry"
            busy={isFetching}
            onClick={() => {
              void refetch()
            }}
          >
            {t('notifications.refresh')}
          </Button>
        ) : undefined
      }
      done={{ label: t('notifications.close'), onClick: () => onOpenChange(false) }}
    >
      {open && (
        <div className="p-4">
          <ReleaseAnnouncementsContent showRefresh={false} />
        </div>
      )}
    </Sheet>
  )
}
