import { render, screen, act } from '@testing-library/react'
import type { HTMLAttributes, ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { getSetupState, onSetupStateChanged, startNewSpace } from '@/api/setup'
import i18n from '@/i18n'
import SetupPage from '@/pages/SetupPage'

// Mock the API
vi.mock('@/api/setup', () => ({
  getSetupState: vi.fn(),
  onSetupStateChanged: vi.fn(() => Promise.resolve(() => {})),
  startNewSpace: vi.fn(),
  startJoinSpace: vi.fn(),
  selectJoinPeer: vi.fn(),
  submitPassphrase: vi.fn(),
  verifyPassphrase: vi.fn(),
  confirmPeerTrust: vi.fn(),
  cancelSetup: vi.fn(),
}))

// Mock react-router-dom
const navigateMock = vi.fn()
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return {
    ...actual,
    useNavigate: () => navigateMock,
  }
})

// Mock framer-motion to avoid animation issues in tests
vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get: () => (props: HTMLAttributes<HTMLDivElement>) => <div {...props} />,
    }
  ),
}))

describe('Setup flow', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    vi.mocked(getSetupState).mockReset()
    vi.mocked(startNewSpace).mockReset()
    vi.mocked(onSetupStateChanged).mockReset()
    vi.mocked(onSetupStateChanged).mockResolvedValue(() => {})
    navigateMock.mockReset()
  })

  it('renders welcome step for SetupState.Welcome', async () => {
    // mock getSetupState() to return 'Welcome'
    vi.mocked(getSetupState).mockResolvedValue('Welcome')

    render(<SetupPage />)

    expect(await screen.findByText('欢迎使用 UniClipboard')).toBeInTheDocument()
    expect(screen.getByText(i18n.t('setup.welcome.subtitle'))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('setup.welcome.create.title'))).toBeInTheDocument()

    await act(async () => {
      await i18n.changeLanguage('en-US')
    })

    expect(await screen.findByText('Welcome to UniClipboard')).toBeInTheDocument()
  })

  it('shows passphrase mismatch error text', async () => {
    // mock getSetupState() to return CreateSpaceInputPassphrase with error
    vi.mocked(getSetupState).mockResolvedValue({
      CreateSpaceInputPassphrase: { error: 'PassphraseMismatch' },
    })

    render(<SetupPage />)

    // Wait for the error message to appear
    expect(
      await screen.findByText(i18n.t('setup.createPassphrase.errors.mismatch'))
    ).toBeInTheDocument()
  })

  it('starts new space when clicking create CTA', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(startNewSpace).mockResolvedValue({
      CreateSpaceInputPassphrase: { error: null },
    })
    render(<SetupPage />)

    const ctaText = await screen.findByText(i18n.t('setup.welcome.create.cta'))
    const createBtn = ctaText.closest('button')
    expect(createBtn).toBeTruthy()

    if (!createBtn) {
      throw new Error('Create CTA button not found')
    }

    await act(async () => {
      createBtn.click()
    })

    expect(startNewSpace).toHaveBeenCalled()
  })

  it('uses non-scrollable main layout on welcome step', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')

    const { container } = render(<SetupPage />)
    await screen.findByText('欢迎使用 UniClipboard')

    const mainContainer = container.querySelector('main')
    expect(mainContainer).toBeTruthy()
    expect(mainContainer).toHaveClass('overflow-hidden')
    expect(mainContainer).not.toHaveClass('overflow-y-auto')
  })

  it('does not show step dot indicator on welcome step', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')

    const { container } = render(<SetupPage />)
    await screen.findByText('欢迎使用 UniClipboard')

    const dots = container.querySelectorAll('[data-testid^="dot-"]')
    expect(dots.length).toBe(0)
  })

  it('cleans listener when registration resolves after unmount', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')

    const stopListening = vi.fn()
    let resolveRegistration: ((value: () => void) => void) | null = null
    const registrationPromise = new Promise<() => void>(resolve => {
      resolveRegistration = resolve
    })
    vi.mocked(onSetupStateChanged).mockImplementation(() => registrationPromise)

    const view = render(<SetupPage />)
    view.unmount()

    expect(stopListening).not.toHaveBeenCalled()

    await act(async () => {
      if (!resolveRegistration) {
        throw new Error('listener registration resolver missing')
      }
      resolveRegistration(stopListening)
      await Promise.resolve()
    })

    expect(stopListening).toHaveBeenCalledTimes(1)
  })
})
