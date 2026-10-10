import { Search } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  HistoryTagColorDto,
  HistoryTagDto,
  HistoryTagRenameResultDto,
} from '@/api/daemon/history-tags'
import { MAX_TAG_NAME_LENGTH } from '@/components/history/detail/history-tag-suggestions'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import type { SearchTagOption } from '@/lib/search-tags'
import { DEFAULT_TAG_COLOR } from '@/lib/tag-colors'
import { libraryTags, similarTagOf, sortTags, TAG_SORTS, type TagSort } from './history-tag-library'
import HistoryTagManagerBulkBar from './HistoryTagManagerBulkBar'
import HistoryTagManagerRow from './HistoryTagManagerRow'

interface HistoryTagManagerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** This device's tags, most used first. */
  tags: HistoryTagDto[]
  /** Search tag counts, for the builtin rows. */
  searchTags: SearchTagOption[]
  /** The sidebar's tags (the daemon's tag layout); `null` while unknown. */
  sidebarTagIds: readonly string[] | null
  onCreate: (name: string, color?: HistoryTagColorDto) => Promise<boolean>
  onSetColor: (tagId: string, color: HistoryTagColorDto | null) => Promise<boolean>
  onSetInSidebar: (tagId: string, inSidebar: boolean) => Promise<boolean>
  onRename: (tagId: string, name: string) => Promise<HistoryTagRenameResultDto | null>
  onMerge: (targetTagId: string, sourceTagIds: string[]) => Promise<boolean>
  onDelete: (tagIds: string[]) => Promise<boolean>
  onShowItems: (tagId: string) => void
  /** The control to refocus on close; `null` leaves focus where it is. */
  returnFocusTo?: HTMLElement | null
}

/**
 * The Library's tag manager (HManage.dc.html, tags tab): the builtin tags and
 * this device's tags, to filter, sort, add, recolor, show in the sidebar,
 * rename, merge and delete — local tags one at a time or several checked at
 * once. Smart views are not part of this round.
 */
function HistoryTagManager({
  open,
  onOpenChange,
  tags,
  searchTags,
  sidebarTagIds,
  onCreate,
  onSetColor,
  onSetInSidebar,
  onRename,
  onMerge,
  onDelete,
  onShowItems,
  returnFocusTo = null,
}: HistoryTagManagerProps) {
  const { t } = useTranslation()
  const [filter, setFilter] = useState('')
  const [sort, setSort] = useState<TagSort>('mostUsed')
  const [adding, setAdding] = useState(false)
  const [checkedIds, setCheckedIds] = useState<ReadonlySet<string>>(new Set())
  useShortcutLayer({ layer: 'modal', scope: 'modal', enabled: open })

  const rows = libraryTags(tags, searchTags, id => t(`history.type.${id}`))
  const local = rows.filter(tag => !tag.builtin)
  const needle = filter.trim().toLocaleLowerCase()
  const matching = needle
    ? rows.filter(tag => tag.name?.toLocaleLowerCase().includes(needle))
    : rows
  // Builtin tags trail the user's own, whatever the order: they carry fewer
  // actions, and among the most used they would read as broken tags.
  const shown = [
    ...sortTags(
      matching.filter(tag => !tag.builtin),
      sort
    ),
    ...matching.filter(tag => tag.builtin),
  ]
  const unused = local.filter(tag => tag.entryCount === 0).length
  const named = local.filter(tag => tag.name)
  const sidebar = new Set(sidebarTagIds)
  // Checks of tags that are gone (merged, deleted) drop out on their own.
  const checked = local.filter(tag => checkedIds.has(tag.tagId))

  const toggleChecked = (tagId: string) =>
    setCheckedIds(current => {
      const next = new Set(current)
      if (!next.delete(tagId)) next.add(tagId)
      return next
    })
  const clearChecked = () => setCheckedIds(new Set())

  const submitNew = async (name: string) => {
    if (name.trim() === '') {
      setAdding(false)
      return
    }
    if (await onCreate(name, DEFAULT_TAG_COLOR)) setAdding(false)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={next => {
        if (!next) clearChecked()
        onOpenChange(next)
      }}
    >
      <DialogContent
        className="h-150 gap-0 overflow-hidden p-0 sm:max-w-190"
        // Not Base UI's default (the previously focused element): a pointer
        // click in WebKit focuses nothing, so that lands on an unrelated row.
        finalFocus={() => (returnFocusTo?.isConnected ? returnFocusTo : false)}
      >
        <div className="flex h-15 shrink-0 items-center gap-4 border-b border-border/60 pl-6 pr-14">
          <DialogTitle>{t('history.tags.libraryTitle')}</DialogTitle>
          <span className="rounded-[0.625rem] bg-muted p-0.75">
            <span className="flex h-7.5 items-center rounded-lg bg-background px-3.5 text-ui-body font-semibold shadow-xs">
              {t('history.tags.managerTitle', { n: rows.length })}
            </span>
          </span>
        </div>
        <div className="flex h-14 shrink-0 items-center gap-2.5 px-6">
          <label className="flex h-9 flex-1 items-center gap-2 rounded-[0.625rem] bg-muted px-3">
            <Search className="size-3.5 text-muted-foreground" aria-hidden="true" />
            <input
              type="text"
              aria-label={t('history.tags.filterLabel')}
              placeholder={t('history.tags.filterPlaceholder', { n: rows.length })}
              value={filter}
              onChange={event => setFilter(event.target.value)}
              className="min-w-0 flex-1 bg-transparent text-ui-body outline-none placeholder:text-muted-foreground"
            />
          </label>
          <Select value={sort} onValueChange={setSort} aria-label={t('history.tags.sortLabel')}>
            {/* Paired with "+ New tag": same height, pill shape and padding. */}
            <SelectTrigger
              className="shrink-0 gap-1.5 rounded-full px-3.5 font-medium"
              aria-label={t('history.tags.sortLabel')}
            >
              <SelectValue>{t(`history.tags.sort.${sort}`)}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              {TAG_SORTS.map(option => (
                <SelectItem key={option} value={option}>
                  {t(`history.tags.sort.${option}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="h-9 shrink-0 rounded-full bg-foreground px-3.5 text-ui-body font-medium text-background hover:bg-foreground/90"
          >
            + {t('history.tags.newTag')}
          </button>
        </div>
        <div className="flex h-7.5 shrink-0 items-center gap-3 border-y border-border/60 bg-muted/40 px-6 text-ui-caption font-semibold uppercase text-muted-foreground">
          <span className="w-4.5 shrink-0" />
          <span className="flex-1">{t('history.tags.columnName')}</span>
          <span className="w-22.5 text-right">{t('history.tags.columnItems')}</span>
          <span className="w-27.5 shrink-0 text-center">{t('history.tags.columnInSidebar')}</span>
          <span className="w-7 shrink-0" />
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {adding && (
            <div className="flex h-11 items-center gap-3 border-b border-border/50 px-6">
              <span className="w-4.5 shrink-0 text-center text-muted-foreground">#</span>
              <input
                type="text"
                aria-label={t('history.tags.newTagLabel')}
                maxLength={MAX_TAG_NAME_LENGTH}
                autoFocus
                onKeyDown={event => {
                  if (event.key === 'Enter') void submitNew(event.currentTarget.value)
                  if (event.key === 'Escape') {
                    event.preventDefault()
                    event.stopPropagation()
                    setAdding(false)
                  }
                }}
                onBlur={() => setAdding(false)}
                className="h-7 min-w-0 flex-1 rounded-md border border-border bg-background px-2 text-ui-body outline-none focus:border-foreground"
              />
            </div>
          )}
          {shown.map(tag => (
            <HistoryTagManagerRow
              key={tag.tagId}
              tag={tag}
              similar={tag.builtin ? null : similarTagOf(tag, local)}
              mergeTargets={named.filter(other => other.tagId !== tag.tagId)}
              inSidebar={sidebar.has(tag.tagId)}
              onSetInSidebar={(tagId, next) => void onSetInSidebar(tagId, next)}
              onSetColor={(tagId, color) => void onSetColor(tagId, color)}
              checked={checkedIds.has(tag.tagId)}
              anyChecked={checked.length > 0}
              onToggleChecked={toggleChecked}
              onRename={onRename}
              onMerge={onMerge}
              onDelete={onDelete}
              onShowItems={onShowItems}
            />
          ))}
          {tags.length === 0 && !adding && (
            <p className="px-6 py-8 text-center text-ui-body text-muted-foreground">
              {t('history.tags.managerEmpty')}
            </p>
          )}
        </div>
        {checked.length > 0 ? (
          <HistoryTagManagerBulkBar
            checked={checked}
            mergeTargets={named}
            onMerge={async (targetTagId, sourceTagIds) => {
              if (await onMerge(targetTagId, sourceTagIds)) clearChecked()
            }}
            onDelete={async tagIds => {
              if (await onDelete(tagIds)) clearChecked()
            }}
            onClear={clearChecked}
          />
        ) : (
          <div className="flex h-12 shrink-0 items-center gap-3 border-t border-border/60 bg-muted/40 px-6 text-ui-caption text-muted-foreground">
            <span className="flex-1">
              {t('history.tags.managerFooter', {
                n: rows.length,
                sidebar: rows.filter(tag => sidebar.has(tag.tagId)).length,
                unused,
              })}
            </span>
            <span>{t('history.tags.managerHint')}</span>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

export default HistoryTagManager
