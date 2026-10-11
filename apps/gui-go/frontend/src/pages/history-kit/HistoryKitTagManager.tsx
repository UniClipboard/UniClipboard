import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  HistoryTagColorDto,
  HistoryTagDto,
  HistoryTagRenameResultDto,
} from '@/api/daemon/history-tags'
import { MAX_TAG_NAME_LENGTH } from '@/components/history/detail/history-tag-suggestions'
import {
  libraryTags,
  similarTagOf,
  sortTags,
  TAG_SORTS,
  type TagSort,
} from '@/components/history/tags/history-tag-library'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import type { SearchTagOption } from '@/lib/search-tags'
import { DEFAULT_TAG_COLOR, TAG_COLORS } from '@/lib/tag-colors'
import {
  Button,
  SearchField,
  Select,
  Sheet,
  Switch,
  Table,
  Text,
  TextField,
  type MenuEntry,
} from '@/ui-kit'
import { HistoryKitTagNameCell } from './HistoryKitTagNameCell'

const RENAME = 'rename'
const SHOW = 'show'
const DELETE = 'delete'
const MERGE = 'merge:'
const COLOR = 'color:'
const NEW_ROW = 'new'

interface HistoryKitTagManagerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  tags: HistoryTagDto[]
  searchTags: SearchTagOption[]
  sidebarTagIds: readonly string[] | null
  tagColors: Readonly<Record<string, HistoryTagColorDto>>
  onCreate: (name: string, color?: HistoryTagColorDto) => Promise<boolean>
  onSetColor: (tagId: string, color: HistoryTagColorDto | null) => Promise<boolean>
  onSetInSidebar: (tagId: string, inSidebar: boolean) => Promise<boolean>
  onRename: (tagId: string, name: string) => Promise<HistoryTagRenameResultDto | null>
  onMerge: (targetTagId: string, sourceTagIds: string[]) => Promise<boolean>
  onDelete: (tagIds: string[]) => Promise<boolean>
  onShowItems: (tagId: string) => void
}

/**
 * Manage Tags (design MC3): a sheet with this device's tags and the built-in ones in a table with
 * their item counts and sidebar switches. The row menu renames, merges (similar tag first), shows
 * the items, recolours and deletes (the items stay); + adds a tag, − deletes the selected ones.
 * The design's Smart Views tab is not shown: the Engine has no saved searches. Custom colours from
 * the existing manager are kept on tags but only palette colours are offered here.
 */
export function HistoryKitTagManager({
  open,
  onOpenChange,
  tags,
  searchTags,
  sidebarTagIds,
  tagColors,
  onCreate,
  onSetColor,
  onSetInSidebar,
  onRename,
  onMerge,
  onDelete,
  onShowItems,
}: HistoryKitTagManagerProps) {
  const { t } = useTranslation()
  const [filter, setFilter] = useState('')
  const [sort, setSort] = useState<TagSort>('mostUsed')
  const [selection, setSelection] = useState<string[]>([])
  const [renaming, setRenaming] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const [newName, setNewName] = useState('')
  useShortcutLayer({ layer: 'modal', scope: 'modal', enabled: open })

  const rows = libraryTags(tags, searchTags, id => t(`history.type.${id}`))
  const local = rows.filter(tag => !tag.builtin)
  const needle = filter.trim().toLocaleLowerCase()
  const matching = needle
    ? rows.filter(tag => tag.name?.toLocaleLowerCase().includes(needle))
    : rows
  // Built-in tags trail this device's own: they carry fewer actions.
  const shown = [
    ...sortTags(
      matching.filter(tag => !tag.builtin),
      sort
    ),
    ...matching.filter(tag => tag.builtin),
  ]
  const sidebar = new Set(sidebarTagIds)
  const byId = new Map(rows.map(tag => [tag.tagId, tag]))
  const label = (tagId: string) => `#${byId.get(tagId)?.name ?? t('history.tags.unreadable')}`
  const localIds = (ids: string[]) => ids.filter(id => byId.get(id) && !byId.get(id)!.builtin)

  const submitNew = async (name: string) => {
    if (name.trim() === '') {
      setAdding(false)
      return
    }
    if (await onCreate(name.trim(), DEFAULT_TAG_COLOR)) {
      setAdding(false)
      setNewName('')
    }
  }
  const remove = async (ids: string[]) => {
    const doomed = localIds(ids)
    if (doomed.length > 0 && (await onDelete(doomed))) setSelection([])
  }

  const menu = (ids: string[]): MenuEntry[] => {
    const own = localIds(ids)
    const single = ids.length === 1 ? byId.get(ids[0]) : undefined
    const targets = local.filter(tag => tag.name && !ids.includes(tag.tagId))
    const similar = single && !single.builtin ? similarTagOf(single, local) : null
    const ordered = similar
      ? [similar, ...targets.filter(tag => tag.tagId !== similar.tagId)]
      : targets
    const entries: MenuEntry[] = []
    if (single && !single.builtin) {
      entries.push({ id: RENAME, label: t('history.tags.rename'), icon: 'edit' })
    }
    if (own.length > 0 && ordered.length > 0) {
      entries.push({
        id: 'merge',
        label: t('history.tags.mergeInto'),
        children: ordered.map(target => ({
          id: `${MERGE}${target.tagId}`,
          label: `#${target.name}`,
          count: target.tagId === similar?.tagId ? t('history.tags.suggested') : undefined,
          swatch: undefined,
        })),
      })
    }
    if (single) entries.push({ id: SHOW, label: t('history.tags.showItems'), icon: 'search' })
    if (single && !single.builtin) {
      entries.push({ id: 'colors-separator', kind: 'separator' })
      for (const color of TAG_COLORS) {
        entries.push({
          id: `${COLOR}${color}`,
          label: t(`history.tags.colors.${color}`),
          swatch: color,
          checked: tagColors[single.tagId] === color,
        })
      }
    }
    if (own.length > 0) {
      entries.push({ id: 'delete-separator', kind: 'separator' })
      entries.push({
        id: DELETE,
        label:
          own.length === 1
            ? t('history.tags.delete')
            : t('history.tags.deleteChecked', { n: own.length }),
        icon: 'delete',
        destructive: true,
      })
    }
    return entries
  }
  const act = (action: string, ids: string[]) => {
    if (action === RENAME) setRenaming(ids[0])
    else if (action === SHOW) onShowItems(ids[0])
    else if (action === DELETE) void remove(ids)
    else if (action.startsWith(MERGE)) {
      void onMerge(action.slice(MERGE.length), localIds(ids)).then(ok => ok && setSelection([]))
    } else if (action.startsWith(COLOR)) void onSetColor(ids[0], action.slice(COLOR.length))
  }

  const tableRows = [
    ...(adding
      ? [
          {
            id: NEW_ROW,
            cells: {
              name: (
                <TextField
                  label={t('history.tags.newTagLabel')}
                  value={newName}
                  onValueChange={setNewName}
                  maxLength={MAX_TAG_NAME_LENGTH}
                  autoFocus
                  onSubmit={value => void submitNew(value)}
                  onCancel={() => setAdding(false)}
                />
              ),
              items: null,
              sidebar: null,
            },
          },
        ]
      : []),
    ...shown.map(tag => ({
      id: tag.tagId,
      cells: {
        name: (
          <HistoryKitTagNameCell
            key={renaming === tag.tagId ? 'renaming' : 'name'}
            tag={tag}
            similarName={tag.builtin ? null : (similarTagOf(tag, local)?.name ?? null)}
            renaming={renaming === tag.tagId}
            onRename={async name => {
              const result = await onRename(tag.tagId, name)
              if (result === null) return t('history.tags.failed')
              return result.kind === 'name_conflict'
                ? t('history.tags.nameTaken', { name: `#${name}` })
                : null
            }}
            onRenameEnd={() => setRenaming(null)}
          />
        ),
        items: <Text>{tag.entryCount.toLocaleString()}</Text>,
        sidebar: (
          <Switch
            label={t('history.tags.showInSidebar', { name: label(tag.tagId) })}
            labelHidden
            checked={sidebar.has(tag.tagId)}
            onCheckedChange={next => void onSetInSidebar(tag.tagId, next)}
          />
        ),
      },
    })),
  ]

  return (
    <Sheet
      open={open}
      onOpenChange={next => {
        if (!next) {
          setSelection([])
          setRenaming(null)
          setAdding(false)
        }
        onOpenChange(next)
      }}
      title={t('history.kit.manageTitle')}
      header={
        <>
          <Text role="callout" tone="secondary">
            {t('history.tags.managerTitle', { n: rows.length })}
          </Text>
          <span className="flex-1" />
          <Select
            label={t('history.tags.sortLabel')}
            value={sort}
            onValueChange={value => setSort(value as TagSort)}
            items={TAG_SORTS.map(option => ({
              value: option,
              label: t(`history.tags.sort.${option}`),
            }))}
          />
          <SearchField
            label={t('history.tags.filterLabel')}
            placeholder={t('history.tags.filterPlaceholder', { n: rows.length })}
            value={filter}
            onValueChange={setFilter}
          />
        </>
      }
      footer={
        <>
          <Button
            icon="add"
            label={t('history.tags.newTag')}
            onClick={() => {
              setNewName('')
              setAdding(true)
            }}
          />
          <Button
            icon="delete"
            label={t('history.kit.deleteSelectedTags')}
            disabled={localIds(selection).length === 0}
            onClick={() => void remove(selection)}
          />
          <Text role="caption" tone="secondary">
            {t('history.tags.managerFooter', {
              n: rows.length,
              sidebar: rows.filter(tag => sidebar.has(tag.tagId)).length,
              unused: local.filter(tag => tag.entryCount === 0).length,
            })}
          </Text>
        </>
      }
      done={{ label: t('history.composite.done'), onClick: () => onOpenChange(false) }}
    >
      <Table
        label={t('history.tags.title')}
        columns={[
          { id: 'name', label: t('history.tags.columnName'), fill: true },
          { id: 'items', label: t('history.tags.columnItems'), align: 'end' },
          { id: 'sidebar', label: t('history.tags.columnInSidebar'), align: 'center' },
        ]}
        rows={tableRows}
        selection={selection.filter(id => byId.has(id))}
        onSelectionChange={setSelection}
        onDelete={ids => void remove(ids)}
        contextMenu={{ items: menu, onAction: act }}
        empty={tags.length === 0 ? t('history.tags.managerEmpty') : undefined}
      />
    </Sheet>
  )
}
