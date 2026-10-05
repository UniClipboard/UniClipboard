import { MoreHorizontal } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto, HistoryTagRenameResultDto } from '@/api/daemon/history-tags'
import { MAX_TAG_NAME_LENGTH } from '@/components/history/detail/history-tag-suggestions'
import { useTagColor } from '@/components/history/tags/tag-colors-context'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from '@/components/motion/context-menu'
import { Checkbox } from '@/components/ui/checkbox'
import { Switch } from '@/components/ui/switch'
import { tagTint } from '@/lib/tag-colors'
import { cn } from '@/lib/utils'
import type { LibraryTag } from './history-tag-library'
import HistoryTagColorMenu from './HistoryTagColorMenu'
import HistoryTagCustomColor from './HistoryTagCustomColor'

interface HistoryTagManagerRowProps {
  tag: LibraryTag
  /** The tag this one likely duplicates; its Merge suggests it first. */
  similar: LibraryTag | null
  /** The other tags this one can merge into (named local ones only). */
  mergeTargets: LibraryTag[]
  /** Whether the sidebar shows this tag (the daemon's tag layout). */
  inSidebar: boolean
  onSetInSidebar: (tagId: string, inSidebar: boolean) => void
  onSetColor: (tagId: string, color: HistoryTagColorDto) => void
  /** Several-at-once selection: this row is checked / any row is. The box
   * stands in for the colour dot on hover and while anything is checked. */
  checked: boolean
  anyChecked: boolean
  onToggleChecked: (tagId: string) => void
  onRename: (tagId: string, name: string) => Promise<HistoryTagRenameResultDto | null>
  onMerge: (targetTagId: string, sourceTagIds: string[]) => Promise<boolean>
  onDelete: (tagIds: string[]) => Promise<boolean>
  onShowItems: (tagId: string) => void
}

/**
 * One tag in the manager: dot (or its check box), name (renamed in place), a
 * hint when it looks like another tag or holds nothing, item count, its "In
 * sidebar" switch and its menu. A name taken by another tag offers a merge
 * instead. A builtin tag can only be recolored, shown or pinned; a tag whose
 * name cannot be read can only be recolored or deleted.
 */
function HistoryTagManagerRow({
  tag,
  similar,
  mergeTargets,
  inSidebar,
  onSetInSidebar,
  onSetColor,
  checked,
  anyChecked,
  onToggleChecked,
  onRename,
  onMerge,
  onDelete,
  onShowItems,
}: HistoryTagManagerRowProps) {
  const { t } = useTranslation()
  const color = useTagColor(tag.tagId)
  const [renaming, setRenaming] = useState(false)
  const [customColorOpen, setCustomColorOpen] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const menuButtonRef = useRef<HTMLButtonElement>(null)
  const [conflictWith, setConflictWith] = useState<LibraryTag | null>(null)
  const label = `#${tag.name ?? t('history.tags.unreadable')}`
  const readable = tag.name !== null
  const editable = readable && !tag.builtin
  // The likely duplicate leads the Merge list.
  const targets = similar
    ? [similar, ...mergeTargets.filter(other => other.tagId !== similar.tagId)]
    : mergeTargets

  const submitRename = async (name: string) => {
    if (name.trim() === '' || name.trim() === tag.name) {
      setRenaming(false)
      return
    }
    const result = await onRename(tag.tagId, name)
    if (!result) return
    setRenaming(false)
    setConflictWith(
      result.kind === 'name_conflict'
        ? (mergeTargets.find(other => other.tagId === result.existingTagId) ?? null)
        : null
    )
  }

  return (
    <div
      data-testid="tag-manager-row"
      className={cn(
        'group border-b border-border/50',
        (checked || menuOpen) && 'bg-history-accent-soft'
      )}
    >
      <div className="flex h-11 items-center gap-3 px-6">
        <span className="relative flex w-4.5 shrink-0 items-center justify-center">
          {!tag.builtin && (
            <Checkbox
              checked={checked}
              onCheckedChange={() => onToggleChecked(tag.tagId)}
              aria-label={t('history.tags.checkTag', { name: label })}
              className={cn(
                'size-4 rounded-[4px]',
                !anyChecked && 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100'
              )}
            />
          )}
          {(tag.builtin || !anyChecked) && (
            <span
              aria-hidden="true"
              style={tagTint(color).style}
              className={cn(
                'pointer-events-none absolute size-2.5 rounded-full',
                !tag.builtin && 'group-hover:hidden group-focus-within:hidden',
                tagTint(color).dot
              )}
            />
          )}
        </span>
        <div className="flex min-w-0 flex-1 items-center gap-2.5">
          {renaming ? (
            <input
              type="text"
              aria-label={t('history.tags.renameLabel', { name: label })}
              defaultValue={tag.name ?? ''}
              maxLength={MAX_TAG_NAME_LENGTH}
              autoFocus
              onKeyDown={event => {
                if (event.key === 'Enter') void submitRename(event.currentTarget.value)
                if (event.key === 'Escape') {
                  event.preventDefault()
                  event.stopPropagation()
                  setRenaming(false)
                }
              }}
              onBlur={() => setRenaming(false)}
              className="h-7 min-w-0 flex-1 rounded-md border border-border bg-background px-2 text-ui-body outline-none focus:border-foreground"
            />
          ) : (
            <span className={cn('truncate text-ui-body font-medium', !readable && 'italic')}>
              {label}
            </span>
          )}
          {!renaming && (tag.builtin || similar || tag.entryCount === 0) && (
            <span className="min-w-0 truncate font-mono text-ui-caption text-muted-foreground">
              {tag.builtin
                ? t('history.tags.builtin')
                : similar
                  ? t('history.tags.similarTo', { name: `#${similar.name}` })
                  : t('history.tags.unused')}
            </span>
          )}
        </div>
        <span className="w-22.5 shrink-0 text-right text-ui-body tabular-nums text-foreground/80">
          {tag.entryCount}
        </span>
        <span className="flex w-27.5 shrink-0 justify-center">
          <Switch
            checked={inSidebar}
            className={inSidebar ? 'bg-tag-green' : undefined}
            onCheckedChange={next => onSetInSidebar(tag.tagId, next)}
            aria-label={t('history.tags.showInSidebar', { name: label })}
          />
        </span>
        <ContextMenu onOpenChange={setMenuOpen}>
          <ContextMenuTrigger activation="click">
            <button
              ref={menuButtonRef}
              type="button"
              aria-label={t('history.tags.actionsFor', { name: label })}
              className={cn(
                'flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted',
                menuOpen && 'bg-muted'
              )}
            >
              <MoreHorizontal className="size-4" aria-hidden="true" />
            </button>
          </ContextMenuTrigger>
          <ContextMenuContent
            ariaLabel={t('history.tags.actionsFor', { name: label })}
            align="end"
            className="w-55"
          >
            {editable && (
              <ContextMenuItem onSelect={() => setRenaming(true)}>
                {t('history.tags.rename')}
              </ContextMenuItem>
            )}
            <HistoryTagColorMenu
              color={color}
              onChange={next => onSetColor(tag.tagId, next)}
              // After the menu has closed, so its dismissal does not close
              // the picker straight away.
              onCustom={() => requestAnimationFrame(() => setCustomColorOpen(true))}
            />
            {editable && (
              <>
                <ContextMenuSub>
                  <ContextMenuSubTrigger
                    textValue={t('history.tags.mergeInto')}
                    disabled={targets.length === 0}
                  >
                    <span className="flex-1">{t('history.tags.mergeInto')}</span>
                    {/* The suggested target: the tag this one likely duplicates. */}
                    {similar && (
                      <span className="min-w-0 truncate text-ui-caption text-muted-foreground">
                        #{similar.name}
                      </span>
                    )}
                  </ContextMenuSubTrigger>
                  <ContextMenuSubContent className="max-h-72 w-52 overflow-y-auto">
                    {targets.map(target => (
                      <ContextMenuItem
                        key={target.tagId}
                        textValue={target.name ?? target.tagId}
                        onSelect={() => void onMerge(target.tagId, [tag.tagId])}
                      >
                        <span className="min-w-0 flex-1 truncate">#{target.name}</span>
                        {target === similar && (
                          <span className="shrink-0 text-ui-caption text-muted-foreground">
                            {t('history.tags.suggested')}
                          </span>
                        )}
                      </ContextMenuItem>
                    ))}
                  </ContextMenuSubContent>
                </ContextMenuSub>
              </>
            )}
            {readable && (
              <ContextMenuItem onSelect={() => onShowItems(tag.tagId)}>
                {t('history.tags.showItems')}
              </ContextMenuItem>
            )}
            {!tag.builtin && (
              <>
                <ContextMenuSeparator />
                <ContextMenuItem
                  tone="destructive"
                  className="font-semibold text-tag-orange-ink"
                  onSelect={() => void onDelete([tag.tagId])}
                >
                  {t('history.tags.delete')}
                </ContextMenuItem>
              </>
            )}
            {tag.builtin && (
              <>
                <ContextMenuSeparator />
                <p className="px-2.5 py-1.5 text-ui-caption text-muted-foreground">
                  {t('history.tags.builtinHint')}
                </p>
              </>
            )}
          </ContextMenuContent>
        </ContextMenu>
        <HistoryTagCustomColor
          open={customColorOpen}
          onOpenChange={setCustomColorOpen}
          anchor={menuButtonRef}
          color={color}
          onSave={next => onSetColor(tag.tagId, next)}
        />
      </div>
      {conflictWith && (
        <div className="flex items-center gap-3 px-6 pb-2.5 pl-11.5 text-ui-caption text-muted-foreground">
          <span>{t('history.tags.nameTaken', { name: `#${conflictWith.name}` })}</span>
          <button
            type="button"
            onClick={() => {
              setConflictWith(null)
              void onMerge(conflictWith.tagId, [tag.tagId])
            }}
            className="font-semibold text-foreground underline-offset-2 hover:underline"
          >
            {t('history.tags.mergeIntoName', { name: `#${conflictWith.name}` })}
          </button>
          <button
            type="button"
            onClick={() => setConflictWith(null)}
            className="hover:text-foreground"
          >
            {t('history.tags.cancel')}
          </button>
        </div>
      )}
    </div>
  )
}

export default HistoryTagManagerRow
