import { render, act } from '@testing-library/react'
import type { HTMLAttributes, ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { getSetupState } from '@/api/setup'
import SetupPage from '@/pages/SetupPage'

if (
  typeof globalThis.localStorage === 'undefined' ||
  typeof globalThis.localStorage.getItem !== 'function'
) {
  const store = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value)
      },
      removeItem: (key: string) => {
        store.delete(key)
      },
      clear: () => {
        store.clear()
      },
    },
    configurable: true,
  })
}

if (typeof globalThis.navigator === 'undefined') {
  Object.defineProperty(globalThis, 'navigator', {
    value: { language: 'en-US' },
    configurable: true,
  })
} else if (!('language' in globalThis.navigator)) {
  Object.defineProperty(globalThis.navigator, 'language', {
    value: 'en-US',
    configurable: true,
  })
}

const loadI18n = await import('@/i18n')
const i18n = loadI18n.default

vi.mock('@/api/setup', () => ({
  getSetupState: vi.fn(),
  onSetupStateChanged: vi.fn(() => Promise.resolve(() => {})),
  startNewSpace: vi.fn(),
  startJoinSpace: vi.fn(),
  selectJoinPeer: vi.fn(),
  submitPassphrase: vi.fn(),
  verifyPassphrase: vi.fn(),
  cancelSetup: vi.fn(),
}))

const navigateMock = vi.fn()
vi.mock('react-router-dom', () => ({
  useNavigate: () => navigateMock,
}))

vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get: () => (props: HTMLAttributes<HTMLDivElement>) => <div {...props} />,
    }
  ),
}))

describe('setup-ready-flow', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    ;(getSetupState as Mock).mockReset()
    navigateMock.mockReset()
  })

  it('renders SetupDoneStep when setup state is Completed and allows entering app', async () => {
    const onComplete = vi.fn()
    ;(getSetupState as Mock).mockResolvedValue('Completed')

    const view = render(<SetupPage onCompleteSetup={onComplete} />)

    expect(await view.findByText('初始化完成')).toBeTruthy()
    const enterButton = await view.findByRole('button', { name: '进入 UniClipboard' })

    await act(async () => {
      enterButton.click()
    })

    expect(onComplete).toHaveBeenCalledTimes(1)
    expect(navigateMock).toHaveBeenCalledWith('/', { replace: true })
  })
})
