import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useGetReleaseAnnouncementsQuery } from '@/api/announcements'
import { formatShortcutHint } from '@/lib/shortcut-format'
import type { SidebarNavItem } from '@/ui-kit'

const NOTIFICATIONS = 'notifications'
const MANAGE_DEVICES = 'devices:manage'
const SETTINGS = 'settings'

export interface HistoryManagement {
  items: SidebarNavItem[]
  onSelect: (id: string) => void
}

export function useHistoryManagement(onDevices: () => void, onSettings: () => void) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = useState(false)
  const { currentData } = useGetReleaseAnnouncementsQuery(i18n.language, {
    pollingInterval: 5 * 60 * 1000,
    skipPollingIfUnfocused: true,
    refetchOnMountOrArgChange: true,
    refetchOnFocus: true,
    refetchOnReconnect: true,
  })
  const count = currentData?.length ?? 0
  const management: HistoryManagement = {
    items: [
      {
        id: NOTIFICATIONS,
        label: count
          ? t('notifications.entryWithCount', { count })
          : t('settings.categories.notifications'),
        icon: 'bell',
      },
      { id: MANAGE_DEVICES, label: t('history.kit.manageDevices'), icon: 'devices' },
      {
        id: SETTINGS,
        label: t('history.kit.settings'),
        icon: 'settings',
        shortcutHint: formatShortcutHint('mod+comma'),
      },
    ],
    onSelect: id => {
      if (id === NOTIFICATIONS) setOpen(true)
      else if (id === MANAGE_DEVICES) onDevices()
      else if (id === SETTINGS) onSettings()
    },
  }
  return { management, open, setOpen }
}
