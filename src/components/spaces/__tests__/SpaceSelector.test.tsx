import { configureStore } from '@reduxjs/toolkit'
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Provider } from 'react-redux'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { JoinSpaceProfileRequest, SpaceProfileSummary } from '@/api/daemon/spaces'
import SpaceSelector from '@/components/spaces/SpaceSelector'
import spacesReducer, { fetchSpaces, joinSpace } from '@/store/spacesSlice'

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

const renderSelector = (
  items: SpaceProfileSummary[],
  authoritativeItems: SpaceProfileSummary[] = items
) => {
  listSpacesApi.mockResolvedValueOnce(items).mockResolvedValue(authoritativeItems)
  const store = configureStore({
    reducer: { spaces: spacesReducer },
    preloadedState: {
      spaces: {
        ...spacesReducer(undefined, { type: '@@init' }),
        items,
      },
    },
  })

  render(
    <Provider store={store}>
      <SpaceSelector />
    </Provider>
  )

  return { store }
}

describe('SpaceSelector', () => {
  beforeEach(() => {
    listSpacesApi.mockReset()
    joinSpaceProfileApi.mockReset()
    setActiveSendSpaceApi.mockReset()
    document.elementFromPoint = vi.fn(() => document.body)
  })

  afterEach(async () => {
    cleanup()
    await new Promise(resolve => setTimeout(resolve, 60))
  })

  it('lists every space with display-name fallback and runtime/incoming states', async () => {
    const work = makeSpace('a', { displayName: 'Work', isActiveSend: true })
    const personal = makeSpace('b', { displayName: null, deviceName: 'Surface Laptop' })
    renderSelector([work, personal])

    const workItem = screen.getByRole('listitem', { name: 'Work' })
    const personalItem = screen.getByRole('listitem', { name: 'Surface Laptop' })
    expect(within(workItem).getByText('Runtime: Running')).toBeInTheDocument()
    expect(within(workItem).getByText('Incoming: Receiving')).toBeInTheDocument()
    expect(within(personalItem).getByText('Runtime: Running')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Work/ })).toHaveAttribute('aria-pressed', 'true')
    await waitFor(() => expect(listSpacesApi).toHaveBeenCalledOnce())
  })

  it('switches active-send without removing or stopping another space', async () => {
    const user = userEvent.setup()
    const first = makeSpace('a', { displayName: 'Work', isActiveSend: true })
    const second = makeSpace('b', { displayName: 'Personal' })
    const authoritative = [
      { ...first, isActiveSend: false },
      { ...second, isActiveSend: true },
    ]
    setActiveSendSpaceApi.mockResolvedValue(authoritative[1])
    renderSelector([first, second], authoritative)

    await user.click(screen.getByRole('button', { name: /Personal/ }))

    expect(setActiveSendSpaceApi).toHaveBeenCalledWith('b')
    expect(screen.getAllByRole('listitem')).toHaveLength(2)
    expect(screen.getByRole('listitem', { name: 'Work' })).toHaveTextContent('Runtime: Running')
    expect(screen.getByRole('listitem', { name: 'Personal' })).toHaveTextContent('Runtime: Running')
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Personal/ })).toHaveAttribute(
        'aria-pressed',
        'true'
      )
    )
  })

  it('keeps the previous space visible when a join succeeds', async () => {
    const first = makeSpace('a', { displayName: 'Work', isActiveSend: true })
    const joined = makeSpace('b', { displayName: 'Family' })
    const { store } = renderSelector([first], [first, joined])
    const request: JoinSpaceProfileRequest = {
      code: 'ABCD-1234',
      passphrase: 'correct horse battery staple',
    }
    joinSpaceProfileApi.mockResolvedValue(joined)

    await store.dispatch(joinSpace(request))

    expect(screen.getByRole('listitem', { name: 'Work' })).toBeInTheDocument()
    expect(screen.getByRole('listitem', { name: 'Family' })).toBeInTheDocument()
  })

  it('shows a newer mutation error instead of a stale list error', async () => {
    const first = makeSpace('a', { displayName: 'Work', isActiveSend: true })
    const { store } = renderSelector([first])

    await waitFor(() => expect(listSpacesApi).toHaveBeenCalledOnce())
    listSpacesApi.mockRejectedValue(new Error('daemon unavailable'))
    await act(async () => {
      await store.dispatch(fetchSpaces())
    })
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't load spaces")

    joinSpaceProfileApi.mockRejectedValue(new Error('invitation rejected'))
    await act(async () => {
      await store.dispatch(
        joinSpace({ code: 'ABCD-1234', passphrase: 'correct horse battery staple' })
      )
    })

    expect(screen.getByRole('alert')).toHaveTextContent(
      "Couldn't join the space. Check the invitation and passphrase, then try again."
    )
  })

  it('shows one profile fault only on that space', () => {
    const healthy = makeSpace('a', { displayName: 'Work' })
    const failed = makeSpace('b', {
      displayName: 'Family',
      runtimeState: { state: 'failed' },
      incomingSyncState: { state: 'degraded' },
      lastFault: { category: 'network', messageCode: 'relay_unreachable' },
    })
    renderSelector([healthy, failed])

    const healthyItem = screen.getByRole('listitem', { name: 'Work' })
    const failedItem = screen.getByRole('listitem', { name: 'Family' })
    expect(within(healthyItem).queryByText('Relay is unreachable')).not.toBeInTheDocument()
    expect(within(failedItem).getByText('Relay is unreachable')).toBeInTheDocument()
    expect(within(failedItem).queryByText(/relay_unreachable|network/)).not.toBeInTheDocument()
  })

  it('uses keyboard-operable labelled buttons and opens the real add-space dialog', async () => {
    const user = userEvent.setup()
    const space = makeSpace('a', { displayName: 'Work' })
    setActiveSendSpaceApi.mockResolvedValue({ ...space, isActiveSend: true })
    renderSelector([space], [{ ...space, isActiveSend: true }])

    const selectButton = screen.getByRole('button', { name: /Work/ })
    selectButton.focus()
    await user.keyboard('{Enter}')
    expect(setActiveSendSpaceApi).toHaveBeenCalledWith('a')

    await user.click(screen.getByRole('button', { name: 'Add space' }))
    expect(screen.getByRole('heading', { name: 'Add a space' })).toBeInTheDocument()
  })
})
