import { createContext, use } from 'react'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import { tagTint } from '@/lib/tag-colors'

type TagColors = Readonly<Record<string, HistoryTagColorDto>>

/** No colors known (layout not loaded): every tag is gray. */
export const NO_TAG_COLORS: TagColors = {}

/** Every tag's color from the daemon's tag layout, keyed by tag id. Pages
 * that show tags provide it; without a provider every tag is gray. */
export const TagColorsContext = createContext<TagColors>(NO_TAG_COLORS)

/** A lookup of tag tints, for components that tint many tags. */
export function useTagTints(): (tagId: string) => ReturnType<typeof tagTint> {
  const colors = use(TagColorsContext)
  return tagId => tagTint(colors[tagId])
}

/** The color chosen for the tag `tagId`, if any. */
export function useTagColor(tagId: string): HistoryTagColorDto | undefined {
  return use(TagColorsContext)[tagId]
}
