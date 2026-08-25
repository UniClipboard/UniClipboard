import { configureStore } from '@reduxjs/toolkit'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Provider } from 'react-redux'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SpaceProfileSummary } from '@/api/daemon/spaces'
import AddSpaceDialog from '@/components/spaces/AddSpaceDialog'
import i18n from '@/i18n'
import spacesReducer from '@/store/spacesSlice'

const listSpacesApi = vi.hoisted(() => vi.fn())
const createSpaceProfileApi = vi.hoisted(() => vi.fn())
const joinSpaceProfileApi = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/spaces', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/daemon/spaces')>()
  return {
    ...actual,
    listSpaces: listSpacesApi,
    createSpaceProfile: createSpaceProfileApi,
    joinSpaceProfile: joinSpaceProfileApi,
  }
})

const joinedSpace: SpaceProfileSummary = {
  profileId: 'family',
  spaceId: 'space-family',
  displayName: 'Family',
  deviceName: 'Office PC',
  runtimeState: { state: 'running' },
  incomingSyncState: { state: 'receiving' },
  lastFault: null,
  isActiveSend: true,
}

function renderDialog(onOpenChange = vi.fn(), mutationError: string | null = null) {
  const store = configureStore({
    reducer: { spaces: spacesReducer },
    preloadedState: {
      spaces: {
        ...spacesReducer(undefined, { type: '@@init' }),
        mutationError,
      },
    },
  })
  render(
    <Provider store={store}>
      <AddSpaceDialog open onOpenChange={onOpenChange} />
    </Provider>
  )
  return { store, onOpenChange }
}

describe('AddSpaceDialog', () => {
  beforeEach(async () => {
    listSpacesApi.mockReset()
    createSpaceProfileApi.mockReset()
    joinSpaceProfileApi.mockReset()
    document.elementFromPoint = vi.fn(() => document.body)
    await i18n.changeLanguage('en-US')
  })

  afterEach(async () => {
    cleanup()
    await new Promise(resolve => setTimeout(resolve, 60))
  })

  it.each([
    ['12345678', '1234-5678'],
    ['ABCD-1234', 'ABCD-1234'],
  ])(
    'normalizes invitation input %s before joining and closes after reconciliation',
    async (enteredCode, expectedCode) => {
      const user = userEvent.setup()
      joinSpaceProfileApi.mockResolvedValue(joinedSpace)
      listSpacesApi.mockResolvedValue([joinedSpace])
      const { onOpenChange } = renderDialog()

      await user.type(screen.getByRole('textbox', { name: 'Invitation code' }), enteredCode)
      await user.type(screen.getByLabelText('Space passphrase'), 'correct horse')
      await user.type(screen.getByLabelText('Device name (optional)'), 'Office PC')
      await user.click(screen.getByRole('button', { name: 'Join space' }))

      expect(joinSpaceProfileApi).toHaveBeenCalledWith({
        code: expectedCode,
        passphrase: 'correct horse',
        deviceName: 'Office PC',
      })
      expect(listSpacesApi).toHaveBeenCalledOnce()
      expect(onOpenChange).toHaveBeenCalledWith(false)
    }
  )

  it('keeps join disabled until the invitation contains exactly eight alphanumerics', async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByRole('textbox', { name: 'Invitation code' }), 'ABC-123')
    await user.type(screen.getByLabelText('Space passphrase'), 'correct horse')

    expect(screen.getByRole('button', { name: 'Join space' })).toBeDisabled()
    expect(joinSpaceProfileApi).not.toHaveBeenCalled()
  })

  it('creates a space only when both passphrase fields match', async () => {
    const user = userEvent.setup()
    createSpaceProfileApi.mockResolvedValue(joinedSpace)
    listSpacesApi.mockResolvedValue([joinedSpace])
    renderDialog()

    await user.click(screen.getByRole('button', { name: 'Create a new space' }))
    await user.type(screen.getByLabelText('Space passphrase'), 'correct horse')
    await user.type(screen.getByLabelText('Confirm passphrase'), 'different')
    expect(screen.getByRole('button', { name: 'Create space' })).toBeDisabled()
    expect(screen.getByText('Passphrases do not match')).toBeInTheDocument()

    await user.clear(screen.getByLabelText('Confirm passphrase'))
    await user.type(screen.getByLabelText('Confirm passphrase'), 'correct horse')
    await user.click(screen.getByRole('button', { name: 'Create space' }))

    expect(createSpaceProfileApi).toHaveBeenCalledWith({
      passphrase: 'correct horse',
      passphraseConfirm: 'correct horse',
      deviceName: null,
    })
  })

  it('shows a localized mutation error instead of a raw error key', async () => {
    const user = userEvent.setup()
    joinSpaceProfileApi.mockRejectedValue(new Error('daemon rejected raw payload'))
    listSpacesApi.mockResolvedValue([])
    const { onOpenChange } = renderDialog()

    await user.type(screen.getByRole('textbox', { name: 'Invitation code' }), '12345678')
    await user.type(screen.getByLabelText('Space passphrase'), 'wrong')
    await user.click(screen.getByRole('button', { name: 'Join space' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Couldn't join the space. Check the invitation and passphrase, then try again."
    )
    expect(screen.queryByText('spaces.errors.join')).not.toBeInTheDocument()
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })

  it('does not carry a previous dialog failure into a newly opened form', async () => {
    renderDialog(vi.fn(), 'spaces.errors.join')

    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
  })
})
