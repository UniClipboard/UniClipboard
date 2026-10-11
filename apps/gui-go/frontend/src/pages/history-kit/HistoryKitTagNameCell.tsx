import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { MAX_TAG_NAME_LENGTH } from '@/components/history/detail/history-tag-suggestions'
import type { LibraryTag } from '@/components/history/tags/history-tag-library'
import { useTagColor } from '@/components/history/tags/tag-colors-context'
import { ColorDot, Text, TextField } from '@/ui-kit'
import { kitTagColor } from './history-kit-model'

/** The name column of the tag manager: the tag, a note (built-in, similar, unused) or the rename field. */
export function HistoryKitTagNameCell({
  tag,
  similarName,
  renaming,
  onRename,
  onRenameEnd,
}: {
  tag: LibraryTag
  similarName: string | null
  renaming: boolean
  /** Resolves to an error message, or `null` when the tag was renamed. */
  onRename: (name: string) => Promise<string | null>
  onRenameEnd: () => void
}) {
  const { t } = useTranslation()
  const color = useTagColor(tag.tagId)
  const label = `#${tag.name ?? t('history.tags.unreadable')}`
  const [draft, setDraft] = useState(tag.name ?? '')
  const [error, setError] = useState<string | undefined>()
  if (renaming) {
    return (
      <TextField
        label={t('history.tags.renameLabel', { name: label })}
        value={draft}
        onValueChange={next => {
          setDraft(next)
          setError(undefined)
        }}
        maxLength={MAX_TAG_NAME_LENGTH}
        autoFocus
        error={error}
        onSubmit={async value => {
          if (value.trim() === '' || value.trim() === tag.name) {
            onRenameEnd()
            return
          }
          const failure = await onRename(value.trim())
          if (failure) setError(failure)
          else onRenameEnd()
        }}
        onCancel={onRenameEnd}
      />
    )
  }
  const note = tag.builtin
    ? t('history.tags.builtin')
    : similarName
      ? t('history.tags.similarTo', { name: `#${similarName}` })
      : tag.entryCount === 0
        ? t('history.tags.unused')
        : null
  return (
    <>
      <ColorDot color={tag.builtin ? undefined : kitTagColor(color)} />
      <Text truncate>{label}</Text>
      {note && (
        <Text role="caption" tone="secondary" truncate>
          {note}
        </Text>
      )}
    </>
  )
}
