import { configureStore } from '@reduxjs/toolkit'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  CreateSpaceProfileRequest,
  JoinSpaceProfileRequest,
  SpaceProfileSummary,
} from '@/api/daemon/spaces'
import spacesReducer, {
  createSpace,
  fetchSpaces,
  joinSpace,
  removeSpace,
  selectActiveSendSpace,
} from '@/store/spacesSlice'

const listSpacesApi = vi.hoisted(() => vi.fn())
const createSpaceProfileApi = vi.hoisted(() => vi.fn())
const joinSpaceProfileApi = vi.hoisted(() => vi.fn())
const setActiveSendSpaceApi = vi.hoisted(() => vi.fn())
const deleteSpaceProfileApi = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/spaces', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/daemon/spaces')>()
  return {
    ...actual,
    listSpaces: listSpacesApi,
    createSpaceProfile: createSpaceProfileApi,
    joinSpaceProfile: joinSpaceProfileApi,
    setActiveSendSpace: setActiveSendSpaceApi,
    deleteSpaceProfile: deleteSpaceProfileApi,
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
    listSpacesApi.mockReset()
    createSpaceProfileApi.mockReset()
    joinSpaceProfileApi.mockReset()
    setActiveSendSpaceApi.mockReset()
    deleteSpaceProfileApi.mockReset()
  })

  it('tracks list loading and list errors without discarding the last view', async () => {
    const existing = makeSpace('a')
    listSpacesApi.mockRejectedValue(new Error('daemon unavailable'))
    const store = makeStore([existing])

    const pending = store.dispatch(fetchSpaces())
    expect(store.getState().spaces.listLoading).toBe(false)
    await pending

    expect(store.getState().spaces.items).toEqual([existing])
    expect(store.getState().spaces.listError).toBe('spaces.errors.load')
  })

  it('serializes rapid active-send A→B→C requests and finishes from the daemon list', async () => {
    const spaces = [makeSpace('a'), makeSpace('b'), makeSpace('c')]
    let daemonActive = 'a'
    let inFlight = 0
    let maxInFlight = 0
    const started: string[] = []
    const settle = new Map<string, (succeed: boolean) => void>()

    setActiveSendSpaceApi.mockImplementation(
      (profileId: string) =>
        new Promise<SpaceProfileSummary>((resolve, reject) => {
          started.push(profileId)
          inFlight += 1
          maxInFlight = Math.max(maxInFlight, inFlight)
          settle.set(profileId, succeed => {
            inFlight -= 1
            if (!succeed) {
              reject(new Error('offline'))
              return
            }
            daemonActive = profileId
            resolve(makeSpace(profileId, { isActiveSend: true }))
          })
        })
    )
    listSpacesApi.mockImplementation(() =>
      Promise.resolve(
        spaces.map(space => ({ ...space, isActiveSend: space.profileId === daemonActive }))
      )
    )
    const store = makeStore(
      spaces.map(space => ({ ...space, isActiveSend: space.profileId === daemonActive }))
    )

    const selectA = store.dispatch(selectActiveSendSpace('a'))
    const selectB = store.dispatch(selectActiveSendSpace('b'))
    const selectC = store.dispatch(selectActiveSendSpace('c'))

    await vi.waitFor(() => expect(started).toEqual(['a']))
    settle.get('a')?.(true)
    await vi.waitFor(() => expect(started).toEqual(['a', 'b']))
    settle.get('b')?.(false)
    await vi.waitFor(() => expect(started).toEqual(['a', 'b', 'c']))
    settle.get('c')?.(true)
    await Promise.all([selectA, selectB, selectC])

    expect(maxInFlight).toBe(1)
    expect(listSpacesApi).toHaveBeenCalledTimes(3)
    expect(store.getState().spaces.items.find(space => space.profileId === 'c')?.isActiveSend).toBe(
      true
    )
    expect(store.getState().spaces.items.filter(space => space.isActiveSend)).toHaveLength(1)
  })

  it('replaces local state from GET after create succeeds and after create fails', async () => {
    const request: CreateSpaceProfileRequest = {
      passphrase: 'correct horse battery staple',
      passphraseConfirm: 'correct horse battery staple',
    }
    const first = makeSpace('a', { isActiveSend: true })
    const created = makeSpace('b', { displayName: 'Returned by mutation' })
    const authoritativeCreated = makeSpace('b', { displayName: 'From daemon list' })
    createSpaceProfileApi.mockResolvedValueOnce(created).mockRejectedValueOnce(new Error('busy'))
    listSpacesApi
      .mockResolvedValueOnce([first, authoritativeCreated])
      .mockResolvedValueOnce([first])
    const store = makeStore([first, makeSpace('stale')])

    await store.dispatch(createSpace(request))
    expect(store.getState().spaces.items).toEqual([first, authoritativeCreated])

    await store.dispatch(createSpace(request))
    expect(store.getState().spaces.items).toEqual([first])
    expect(store.getState().spaces.mutationError).toBe('spaces.errors.create')
    expect(listSpacesApi).toHaveBeenCalledTimes(2)
  })

  it('replaces local state from GET after join succeeds and after join fails', async () => {
    const first = makeSpace('a', { isActiveSend: true })
    const joined = makeSpace('b', { displayName: 'Returned by mutation' })
    const authoritativeJoined = makeSpace('b', { displayName: 'From daemon list' })
    const request: JoinSpaceProfileRequest = {
      code: 'ABCD-1234',
      passphrase: 'correct horse battery staple',
    }
    joinSpaceProfileApi.mockResolvedValueOnce(joined).mockRejectedValueOnce(new Error('expired'))
    listSpacesApi.mockResolvedValueOnce([first, authoritativeJoined]).mockResolvedValueOnce([first])
    const store = makeStore([first, makeSpace('stale')])

    await store.dispatch(joinSpace(request))
    expect(store.getState().spaces.items).toEqual([first, authoritativeJoined])

    await store.dispatch(joinSpace(request))
    expect(store.getState().spaces.items).toEqual([first])
    expect(store.getState().spaces.mutationError).toBe('spaces.errors.join')
    expect(listSpacesApi).toHaveBeenCalledTimes(2)
  })

  it('replaces local state from GET after remove succeeds and after remove fails', async () => {
    const first = makeSpace('a', { isActiveSend: true })
    const second = makeSpace('b')
    deleteSpaceProfileApi.mockResolvedValueOnce(second).mockRejectedValueOnce(new Error('locked'))
    listSpacesApi.mockResolvedValueOnce([first]).mockResolvedValueOnce([first, second])
    const store = makeStore([first, second])

    await store.dispatch(removeSpace('b'))
    expect(store.getState().spaces.items).toEqual([first])

    await store.dispatch(removeSpace('b'))
    expect(store.getState().spaces.items).toEqual([first, second])
    expect(store.getState().spaces.mutationError).toBe('spaces.errors.remove')
    expect(listSpacesApi).toHaveBeenCalledTimes(2)
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
