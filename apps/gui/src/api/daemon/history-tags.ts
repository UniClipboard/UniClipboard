/**
 * Daemon history-tag API module — this device's local tags and their entry
 * associations, and the daemon's tag layout (sidebar tags in order, each
 * tag's color). Tags live only in the local history and are never synced.
 *
 * # Endpoints
 * - `GET /history/tags` → list tags with association counts, most used first
 * - `POST /history/tags` → create a tag, or get the one with the same name
 * - `PATCH /history/tags/{tag_id}` → rename (a taken name is a `name_conflict`)
 * - `DELETE /history/tags/{tag_id}` → delete a tag; its entries stay
 * - `POST /history/tags/{tag_id}/entries/add|remove` → batch (≤1000) associate
 * - `POST /history/tags/summary` → which tags a selection carries
 * - `POST /history/tags/{tag_id}/merge` → fold source tags into this one
 * - `GET /history/tags/layout` → the sidebar's tags in order and every color
 * - `PUT /history/tags/layout/sidebar` → replace the sidebar's tags and order
 * - `PUT /history/tags/{tag_id}/color|sidebar` → one tag's color / sidebar place
 *
 * Errors surface as `DaemonApiError`: 400 invalid input, 404 unknown tag,
 * 423 `session_locked` / `content_locked`, 503 `runtime_unavailable` when the
 * profile cannot hold tags.
 */

import {
  addHistoryTagToEntries,
  createHistoryTag as createHistoryTagSdk,
  deleteHistoryTag as deleteHistoryTagSdk,
  getHistoryTagLayout as getHistoryTagLayoutSdk,
  listHistoryTags as listHistoryTagsSdk,
  mergeHistoryTags as mergeHistoryTagsSdk,
  removeHistoryTagFromEntries,
  renameHistoryTag as renameHistoryTagSdk,
  setHistoryTagColor as setHistoryTagColorSdk,
  setHistoryTagInSidebar as setHistoryTagInSidebarSdk,
  setHistoryTagSidebar as setHistoryTagSidebarSdk,
  summarizeHistoryEntryTags,
} from '@/api/generated/sdk.gen'
import type {
  HistoryEntryTagSummaryDto,
  HistoryTagBatchResultDto,
  HistoryTagColorDto,
  HistoryTagCreatedDto,
  HistoryTagDto,
  HistoryTagLayoutDto,
  HistoryTagMergeResultDto,
  HistoryTagRenameResultDto,
} from '@/api/generated/types.gen'
import { daemonClient } from './client'

export type {
  HistoryEntryTagSummaryDto,
  HistoryTagBatchResultDto,
  HistoryTagColorDto,
  HistoryTagCreatedDto,
  HistoryTagDto,
  HistoryTagLayoutDto,
  HistoryTagMergeResultDto,
  HistoryTagRenameResultDto,
}

/** Most entry ids one add/remove/summary call accepts. */
export const MAX_HISTORY_TAG_BATCH = 1000

export async function listHistoryTags(): Promise<HistoryTagDto[]> {
  const envelope = await daemonClient.callSdk(() => listHistoryTagsSdk({ throwOnError: true }))
  return envelope.data
}

/** Create a tag, or get the one with the same name; `color` applies only to a new tag. */
export async function createHistoryTag(
  name: string,
  color?: HistoryTagColorDto
): Promise<HistoryTagCreatedDto> {
  const envelope = await daemonClient.callSdk(() =>
    createHistoryTagSdk({ body: { name, color }, throwOnError: true })
  )
  return envelope.data
}

export async function renameHistoryTag(
  tagId: string,
  name: string
): Promise<HistoryTagRenameResultDto> {
  const envelope = await daemonClient.callSdk(() =>
    renameHistoryTagSdk({ path: { tag_id: tagId }, body: { name }, throwOnError: true })
  )
  return envelope.data
}

export async function deleteHistoryTag(tagId: string): Promise<number> {
  const envelope = await daemonClient.callSdk(() =>
    deleteHistoryTagSdk({ path: { tag_id: tagId }, throwOnError: true })
  )
  return envelope.data.detached
}

/** Chunks larger selections; each chunk applies atomically on its own. */
async function batched(
  entryIds: string[],
  call: (chunk: string[]) => Promise<HistoryTagBatchResultDto>
): Promise<HistoryTagBatchResultDto> {
  const total: HistoryTagBatchResultDto = { changed: 0, unchanged: 0, missingEntryIds: [] }
  for (let i = 0; i < entryIds.length; i += MAX_HISTORY_TAG_BATCH) {
    const result = await call(entryIds.slice(i, i + MAX_HISTORY_TAG_BATCH))
    total.changed += result.changed
    total.unchanged += result.unchanged
    total.missingEntryIds.push(...result.missingEntryIds)
  }
  return total
}

export function addTagToEntries(
  tagId: string,
  entryIds: string[]
): Promise<HistoryTagBatchResultDto> {
  return batched(entryIds, async chunk => {
    const envelope = await daemonClient.callSdk(() =>
      addHistoryTagToEntries({
        path: { tag_id: tagId },
        body: { entryIds: chunk },
        throwOnError: true,
      })
    )
    return envelope.data
  })
}

export function removeTagFromEntries(
  tagId: string,
  entryIds: string[]
): Promise<HistoryTagBatchResultDto> {
  return batched(entryIds, async chunk => {
    const envelope = await daemonClient.callSdk(() =>
      removeHistoryTagFromEntries({
        path: { tag_id: tagId },
        body: { entryIds: chunk },
        throwOnError: true,
      })
    )
    return envelope.data
  })
}

/** Tags on a selection. Larger selections are summarized in chunks and added
 * up: the chunks are disjoint, so `selected` and each `applied` simply sum. */
export async function summarizeEntryTags(entryIds: string[]): Promise<HistoryEntryTagSummaryDto> {
  let selected = 0
  const applied = new Map<string, number>()
  for (let i = 0; i < entryIds.length; i += MAX_HISTORY_TAG_BATCH) {
    const chunk = entryIds.slice(i, i + MAX_HISTORY_TAG_BATCH)
    const envelope = await daemonClient.callSdk(() =>
      summarizeHistoryEntryTags({ body: { entryIds: chunk }, throwOnError: true })
    )
    selected += envelope.data.selected
    for (const tag of envelope.data.tags) {
      applied.set(tag.tagId, (applied.get(tag.tagId) ?? 0) + tag.applied)
    }
  }
  return { selected, tags: [...applied].map(([tagId, count]) => ({ tagId, applied: count })) }
}

export async function mergeHistoryTags(
  targetTagId: string,
  sourceTagIds: string[]
): Promise<HistoryTagMergeResultDto> {
  const envelope = await daemonClient.callSdk(() =>
    mergeHistoryTagsSdk({
      path: { tag_id: targetTagId },
      body: { sourceTagIds },
      throwOnError: true,
    })
  )
  return envelope.data
}

export async function getHistoryTagLayout(): Promise<HistoryTagLayoutDto> {
  const envelope = await daemonClient.callSdk(() => getHistoryTagLayoutSdk({ throwOnError: true }))
  return envelope.data
}

export async function setHistoryTagSidebar(tagIds: string[]): Promise<HistoryTagLayoutDto> {
  const envelope = await daemonClient.callSdk(() =>
    setHistoryTagSidebarSdk({ body: { tagIds }, throwOnError: true })
  )
  return envelope.data
}

/** `null` clears a local tag's color; a builtin tag returns to its default. */
export async function setHistoryTagColor(
  tagId: string,
  color: HistoryTagColorDto | null
): Promise<HistoryTagLayoutDto> {
  const envelope = await daemonClient.callSdk(() =>
    setHistoryTagColorSdk({ path: { tag_id: tagId }, body: { color }, throwOnError: true })
  )
  return envelope.data
}

/** Show a tag in the sidebar (last) or take it out. */
export async function setHistoryTagInSidebar(
  tagId: string,
  inSidebar: boolean
): Promise<HistoryTagLayoutDto> {
  const envelope = await daemonClient.callSdk(() =>
    setHistoryTagInSidebarSdk({ path: { tag_id: tagId }, body: { inSidebar }, throwOnError: true })
  )
  return envelope.data
}
