import { configureStore } from '@reduxjs/toolkit'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { JoinSpaceProfileRequest, SpaceProfileSummary } from '@/api/daemon/spaces'
import spacesReducer, { fetchSpaces, joinSpace, selectActiveSendSpace } from '@/store/spacesSlice'

const listSpacesApi = vi.hoisted(() => vi.fn())
const joinSpaceProfileApi = vi.hoisted(() => vi.fn())
const setActiveSendSpaceApi = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/spaces', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/daemon/spaces')>()
  return {
    ...actual,
    listSpaces: listSpacesApi,
    joinSpaceProfile: joinSpaceProfileApi,
    setActiveSendSpace: setActiveSendSpaceApi,
  }
})

const makeSpace = (
  profileId: string,
  overrides: Partial<SpaceProfileSummary> = {}
): SpaceProfileSummary => ({
  profileId,
  spaceId: `space-${profileId}`,
  displayName: `Space ${profileId}`,
  deviceName: `Device ${profileId}`,
  runtimeState: { state: 'running' },
  incomingSyncState: { state: 'receiving' },
  lastFault: null,
  isActiveSend: false,
  ...overrides,
})

const makeStore = (spaces: SpaceProfileSummary[] = []) =>
  configureStore({
    reducer: { spaces: spacesReducer },
    preloadedState: {
      spaces: {
        ...spacesReducer(undefined, { type: '@@init' }),
        items: spaces,
      },
    },
  })

describe('spacesSlice', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('tracks list loading and list errors without discarding the last view', async () => {
    const existing = makeSpace('a')
    listSpacesApi.mockRejectedValue(new Error('daemon unavailable'))
    const store = makeStore([existing])

    const pending = store.dispatch(fetchSpaces())
    expect(store.getState().spaces.listLoading).toBe(false)
    await pending

    expect(store.getState().spaces.items).toEqual([existing])
    expect(store.getState().spaces.listError).toBe('Failed to load spaces')
  })

  it('optimistically changes only active-send and rolls back the current request on failure', async () => {
    const first = makeSpace('a', { isActiveSend: true })
    const second = makeSpace('b')
    let rejectRequest!: (reason: Error) => void
    setActiveSendSpaceApi.mockReturnValue(
      new Promise((_, reject) => {
        rejectRequest = reject
      })
    )
    const store = makeStore([first, second])

    const selection = store.dispatch(selectActiveSendSpace('b'))
    expect(store.getState().spaces.items).toHaveLength(2)
    expect(store.getState().spaces.items.map(space => space.runtimeState.state)).toEqual([
      'running',
      'running',
    ])
    expect(store.getState().spaces.items.find(space => space.profileId === 'b')?.isActiveSend).toBe(
      true
    )

    rejectRequest(new Error('offline'))
    await selection

    expect(store.getState().spaces.items.find(space => space.profileId === 'a')?.isActiveSend).toBe(
      true
    )
    expect(store.getState().spaces.items.find(space => space.profileId === 'b')?.isActiveSend).toBe(
      false
    )
  })

  it('adds a joined profile without replacing or stopping existing profiles', async () => {
    const first = makeSpace('a', { isActiveSend: true })
    const joined = makeSpace('b')
    const request: JoinSpaceProfileRequest = {
      code: 'ABCD-1234',
      passphrase: 'correct horse battery staple',
    }
    joinSpaceProfileApi.mockResolvedValue(joined)
    const store = makeStore([first])

    await store.dispatch(joinSpace(request))

    expect(store.getState().spaces.items).toEqual([first, joined])
    expect(store.getState().spaces.items[0].runtimeState.state).toBe('running')
  })

  it('keeps a runtime fault attached only to its owning profile', async () => {
    const healthy = makeSpace('a')
    const failed = makeSpace('b', {
      runtimeState: { state: 'failed' },
      incomingSyncState: { state: 'degraded' },
      lastFault: { category: 'network', messageCode: 'relay_unreachable' },
    })
    listSpacesApi.mockResolvedValue([healthy, failed])
    const store = makeStore()

    await store.dispatch(fetchSpaces())

    expect(store.getState().spaces.items[0].lastFault).toBeNull()
    expect(store.getState().spaces.items[1].lastFault).toEqual({
      category: 'network',
      messageCode: 'relay_unreachable',
    })
    expect(store.getState().spaces.listError).toBeNull()
  })
})
