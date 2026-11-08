import { useTranslation } from 'react-i18next'
import { ReleaseAnnouncementsContent } from '@/components/notifications/ReleaseAnnouncementsContent'
import { SettingGroup } from '@/components/setting/SettingGroup'

export default function NotificationsSection() {
  const { t } = useTranslation()
  return (
    <SettingGroup title={t('notifications.title')}>
      <div className="px-1">
        <ReleaseAnnouncementsContent />
      </div>
    </SettingGroup>
  )
}
