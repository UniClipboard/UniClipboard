import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import { deriveBadgeKind } from '@/components/device/connection-channel-utils'
import { sidebarTagRows } from '@/components/history/sidebar/history-sidebar-tags'
import { useLibraryCounts } from '@/hooks/useLibraryCounts'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import { useNow } from '@/hooks/useRelativeTime'
import { useSettingSelector } from '@/hooks/useSetting'
import { isMobileDeviceActive } from '@/lib/mobile-device-status'
import { tagLabel, type SearchTagOption } from '@/lib/search-tags'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { useAppSelector } from '@/store/hooks'
import { SidebarNav, type SidebarNavItem, type SidebarNavSection } from '@/ui-kit'
import { kitTagColor } from './history-kit-model'

const ALL = 'library:all'
const PINNED = 'library:pinned'
const SETTINGS = 'settings'
const TAG = 'tag:'
const DEVICE = 'device:'

interface HistoryKitSidebarProps {
  activeFilter: Filter
  activeTag: string | null
  onSelectLibrary: (filter: Filter) => void
  onSelectTag: (tagId: string | null) => void
  tags: SearchTagOption[]
  /** The daemon's sidebar tag ids in order; `null` while unknown. */
  sidebarTagIds: readonly string[] | null
  tagColors: Readonly<Record<string, HistoryTagColorDto>>
  /** Opens the tag library; absent while local tags are unavailable. */
  onManageTags?: () => void
  /** Total tags in the library, for the "All Tags" row. */
  tagTotal: number
  /** Changes whenever the list changes, so the library counts refresh. */
  countsRevision: unknown
}

/**
 * The window sidebar (design MA2): the Library rows with their counts, the daemon's sidebar tags,
 * the paired devices with their connection, and Settings at the bottom. Smart Views and Recently
 * Deleted from the design are not shown: the Engine has no saved searches and deletes entries
 * permanently.
 */
export function HistoryKitSidebar({
  activeFilter,
  activeTag,
  onSelectLibrary,
  onSelectTag,
  tags,
  sidebarTagIds,
  tagColors,
  onManageTags,
  tagTotal,
  countsRevision,
}: HistoryKitSidebarProps) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const counts = useLibraryCounts(countsRevision)
  const now = useNow()
  const members = useAppSelector(state => state.devices.spaceMembers)
  const mobileDevices = useMobileDeviceList()
  const lanOnly = useSettingSelector(
    ({ setting }) => setting?.network?.allowRelayFallback === false
  )
  const count = (value: number | undefined) => value?.toLocaleString()

  const library: SidebarNavSection = {
    id: 'library',
    items: [
      { id: ALL, label: t('history.sidebar.allItems'), icon: 'history', count: count(counts?.all) },
      { id: PINNED, label: t('history.sidebar.pinned'), icon: 'pin', count: count(counts?.pinned) },
    ],
  }

  const tagRows = sidebarTagIds ? sidebarTagRows(sidebarTagIds, tags) : []
  const tagSection: SidebarNavSection = {
    id: 'tags',
    title: t('history.sidebar.tags'),
    items: tagRows.map(row => ({
      id: `${TAG}${row.id}`,
      label: `#${tagLabel(row, t)}`,
      swatch: kitTagColor(tagColors[row.id]) ?? 'gray',
      // A locked session does not know the counts.
      count: row.count ? count(row.count) : undefined,
    })),
    action: onManageTags && { label: t('history.tags.manage'), icon: 'add', onClick: onManageTags },
    more:
      onManageTags && tagTotal > 0
        ? { label: t('history.tags.allTags', { n: tagTotal }), onClick: onManageTags }
        : undefined,
    placeholder: { text: t('history.tags.sidebarEmpty') },
  }

  const channelLabel: Record<string, string> = {
    lan: t('devices.list.channel.lan'),
    relay: t('devices.list.channel.relay'),
    outOfLan: t('devices.list.channel.outOfLan'),
    offline: t('devices.list.status.offline'),
    unknown: t('devices.list.channel.unknown'),
  }
  const devices: SidebarNavItem[] = [
    ...members.map(member => {
      const kind = member.connected ? deriveBadgeKind(member.channel, lanOnly) : 'offline'
      return {
        id: `${DEVICE}${member.peerId}`,
        label: member.deviceName,
        status: kind === 'lan' || kind === 'relay' ? ('positive' as const) : ('neutral' as const),
        count: channelLabel[kind],
      }
    }),
    ...mobileDevices.map(device => {
      const online = isMobileDeviceActive(device, now)
      return {
        id: `${DEVICE}${device.deviceId}`,
        label: device.label,
        status: online ? ('positive' as const) : ('neutral' as const),
        count: online ? t('devices.list.status.online') : t('devices.list.status.offline'),
      }
    }),
  ]
  const deviceSection: SidebarNavSection = {
    id: 'devices',
    title: t('history.sidebar.devices'),
    items: devices,
    placeholder: {
      action: { label: t('history.kit.pairDevice'), onClick: () => navigate('/devices') },
    },
  }

  const value =
    activeTag !== null
      ? `${TAG}${activeTag}`
      : activeFilter === Filter.Favorited
        ? PINNED
        : activeFilter === Filter.All
          ? ALL
          : undefined

  const select = (id: string) => {
    if (id === ALL) onSelectLibrary(Filter.All)
    else if (id === PINNED) onSelectLibrary(Filter.Favorited)
    else if (id === SETTINGS) navigate('/settings')
    else if (id.startsWith(DEVICE)) navigate('/devices')
    else if (id.startsWith(TAG)) {
      const tagId = id.slice(TAG.length)
      // Choosing the active tag again clears it.
      onSelectTag(activeTag === tagId ? null : tagId)
    }
  }

  return (
    <SidebarNav
      label={t('history.sidebar.library')}
      sections={[library, tagSection, deviceSection]}
      footer={[
        {
          id: SETTINGS,
          label: t('history.kit.settings'),
          icon: 'settings',
          shortcutHint: formatShortcutHint('mod+comma'),
        },
      ]}
      value={value}
      onValueChange={select}
    />
  )
}
