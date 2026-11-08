import { useTranslation } from 'react-i18next'
import { useGetReleaseAnnouncementsQuery, type ReleaseAnnouncement } from '@/api/announcements'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'
import { Button } from '@/ui-kit'

export function ReleaseAnnouncementsContent({ showRefresh = true }: { showRefresh?: boolean }) {
  const { t, i18n } = useTranslation()
  const {
    currentData: items,
    isFetching,
    isError,
    refetch,
  } = useGetReleaseAnnouncementsQuery(i18n.language, {
    refetchOnMountOrArgChange: true,
  })
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-ui-body text-muted-foreground">{t('notifications.description')}</p>
        {showRefresh && (
          <Button
            icon="retry"
            busy={isFetching}
            onClick={() => {
              void refetch()
            }}
          >
            {t('notifications.refresh')}
          </Button>
        )}
      </div>
      {isError && (
        <p role="alert" className="text-ui-body text-muted-foreground">
          {t('notifications.failed')}
        </p>
      )}
      {!items && isFetching && (
        <p role="status" className="text-ui-body text-muted-foreground">
          {t('notifications.loading')}
        </p>
      )}
      {items && <AnnouncementList items={items} />}
    </div>
  )
}

function AnnouncementList({ items }: { items: ReleaseAnnouncement[] }) {
  const { t, i18n } = useTranslation()
  const locale = i18n.language
  if (items.length === 0)
    return (
      <p role="status" className="text-ui-body text-muted-foreground">
        {t('notifications.empty')}
      </p>
    )
  return (
    <div className="space-y-4" data-testid="release-announcements">
      {items.map(item => (
        <article
          key={item.id}
          className="space-y-2 rounded-lg border border-border bg-card p-4 text-card-foreground"
        >
          <div className="flex flex-wrap items-center gap-2 text-ui-caption text-muted-foreground">
            <span className="font-medium">{t(`notifications.severity.${item.severity}`)}</span>
            <time dateTime={item.publishedAt}>
              {new Date(item.publishedAt).toLocaleString(locale)}
            </time>
          </div>
          <h3 className="text-ui-section font-semibold">{item.title}</h3>
          <ReleaseNotes content={item.content} fallback="" hideImages />
          {item.requireAcknowledgement && (
            <p className="text-ui-caption font-medium">{t('notifications.attention')}</p>
          )}
        </article>
      ))}
    </div>
  )
}
