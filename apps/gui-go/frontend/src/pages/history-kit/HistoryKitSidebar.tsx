import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import { deriveBadgeKind } from '@/components/device/connection-channel-utils'
import { sidebarTagRows } from '@/components/history/sidebar/history-sidebar-tags'
import { useLibraryChrome } from '@/contexts/library-chrome-context'
import { useLibraryCounts } from '@/hooks/useLibraryCounts'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import { useNow } from '@/hooks/useRelativeTime'
import { useSettingSelector } from '@/hooks/useSetting'
import { isMobileDeviceActive } from '@/lib/mobile-device-status'
import { tagLabel, type SearchTagOption } from '@/lib/search-tags'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { invitationSummary } from '@/pages/devices-kit/devices-kit-model'
import type { DevicesKitController } from '@/pages/devices-kit/useDevicesKit'
import { useAppSelector } from '@/store/hooks'
import { SidebarNav, type SidebarNavItem, type SidebarNavSection } from '@/ui-kit'
import { kitTagColor } from './history-kit-model'

const ALL = 'library:all'
const PINNED = 'library:pinned'
const SETTINGS = 'settings'
const MANAGE_DEVICES = 'devices:manage'
const TAG = 'tag:'
const DEVICE = 'device:'
const MOBILE_SOURCE = 'mobile_sync:'
const INVITATION = 'devices:invitation'
const DEVICES_COLLAPSED_KEY = 'uc.history.kit.devicesCollapsed'

function readPreference(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function writePreference(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // A preference that cannot be stored just does not survive a restart.
  }
}

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
  /** The source device the history is filtered to (a source id), or `null`. */
  activeSource: string | null
  onSelectSource: (sourceId: string | null) => void
  /** Manage Devices: the sheet, the invitation row. */
  devices: DevicesKitController
}

/**
 * The window sidebar (design MA2): the Library rows with their counts, the daemon's sidebar tags,
 * the paired devices with their connection, and Settings at the bottom. A device row filters the
 * history by that source; the labelled footer entry opens Manage Devices (design DV0). Smart Views and Recently
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
  activeSource,
  onSelectSource,
  devices,
}: HistoryKitSidebarProps) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const chrome = useLibraryChrome()
  const counts = useLibraryCounts(countsRevision)
  const now = useNow()
  const members = useAppSelector(state => state.devices.spaceMembers)
  const mobileDevices = useMobileDeviceList()
  const lanOnly = useSettingSelector(
    ({ setting }) => setting?.network?.allowRelayFallback === false
  )
  const count = (value: number | undefined) => value?.toLocaleString()
  const [devicesCollapsed, setDevicesCollapsed] = useState(
    () => readPreference(DEVICES_COLLAPSED_KEY) === '1'
  )

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
    icon: 'tag',
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
  // One row per device, as in the original canvas (design DV0). A row filters the history by
  // its source; the ids are the search's source ids (`mobile_sync:` for phones).
  const deviceItems: SidebarNavItem[] = [
    ...members.map(member => {
      const kind = member.connected ? deriveBadgeKind(member.channel, lanOnly) : 'offline'
      return {
        id: `${DEVICE}${member.peerId}`,
        label: member.deviceName,
        status:
          kind === 'lan'
            ? ('positive' as const)
            : kind === 'relay'
              ? ('info' as const)
              : ('neutral' as const),
        count: channelLabel[kind],
      }
    }),
    ...mobileDevices.map(device => {
      const online = isMobileDeviceActive(device, now)
      return {
        id: `${DEVICE}${MOBILE_SOURCE}${device.deviceId}`,
        label: device.label,
        status: online ? ('positive' as const) : ('neutral' as const),
        count: online ? t('devices.list.status.online') : t('devices.list.status.offline'),
      }
    }),
    // A live invitation stays here after Manage Devices closes and reopens it on the invitation.
    ...(devices.invitation.active
      ? [
          {
            id: INVITATION,
            label: t('devices.kit.invitation'),
            icon: 'add' as const,
            count: invitationSummary(devices.invitation.state, t).text,
          },
        ]
      : []),
  ]
  const devicesTitle = t('history.sidebar.devices')
  const deviceSection: SidebarNavSection = {
    id: 'devices',
    title: devicesTitle,
    icon: 'devices',
    items: deviceItems,
    collapse: {
      collapsed: devicesCollapsed,
      label: t(devicesCollapsed ? 'history.kit.showSection' : 'history.kit.hideSection', {
        section: devicesTitle,
      }),
      onCollapsedChange: collapsed => {
        setDevicesCollapsed(collapsed)
        writePreference(DEVICES_COLLAPSED_KEY, collapsed ? '1' : '0')
      },
    },
    placeholder: {
      action: { label: t('history.kit.pairDevice'), onClick: devices.invite },
    },
  }

  const value =
    activeTag !== null
      ? `${TAG}${activeTag}`
      : activeSource !== null
        ? `${DEVICE}${activeSource}`
        : activeFilter === Filter.Favorited
          ? PINNED
          : activeFilter === Filter.All
            ? ALL
            : undefined

  const select = (id: string) => {
    chrome.closeDrawer()
    if (id === ALL) onSelectLibrary(Filter.All)
    else if (id === PINNED) onSelectLibrary(Filter.Favorited)
    else if (id === SETTINGS) navigate('/settings')
    else if (id === MANAGE_DEVICES) devices.show()
    else if (id === INVITATION) devices.show({ kind: 'invitation' })
    else if (id.startsWith(DEVICE)) {
      const sourceId = id.slice(DEVICE.length)
      // Choosing the active device again clears the filter, as with a tag.
      onSelectSource(activeSource === sourceId ? null : sourceId)
    } else if (id.startsWith(TAG)) {
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
          id: MANAGE_DEVICES,
          label: t('history.kit.manageDevices'),
          icon: 'devices',
        },
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
