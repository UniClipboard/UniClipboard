/** The specimen boards of the macOS design canvas the prototype replays (`?board=<id>`). */
export const boardIds = ['c1', 'c2', 'c3', 'c4', 'c5', 't3'] as const

export type BoardId = (typeof boardIds)[number]

export function isBoardId(value: string | null): value is BoardId {
  return boardIds.includes(value as BoardId)
}
