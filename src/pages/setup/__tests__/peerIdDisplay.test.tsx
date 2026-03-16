import { render, screen } from '@testing-library/react'
import type { HTMLAttributes, ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { formatPeerIdForDisplay } from '@/lib/utils'
import JoinVerifyPassphraseStep from '@/pages/setup/JoinVerifyPassphraseStep'

vi.mock('framer-motion', () => ({
  motion: new Proxy(
    {},
    {
      get: () => (props: HTMLAttributes<HTMLDivElement>) => <div {...props} />,
    }
  ),
}))

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

describe('setup peer id display', () => {
  let initialLanguage = 'en-US'

  const ensureI18nInitialized = async () => {
    if (i18n.isInitialized) return
    await new Promise<void>(resolve => {
      const handler = () => {
        i18n.off('initialized', handler)
        resolve()
      }
      i18n.on('initialized', handler)
    })
  }

  beforeAll(async () => {
    await ensureI18nInitialized()
    initialLanguage = i18n.language
    await i18n.changeLanguage('en-US')
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  it('JoinVerifyPassphraseStep uses formatPeerIdForDisplay in target device label', () => {
    const peerId = '12D3KooWABCDEFGH'
    renderWithI18n(
      <JoinVerifyPassphraseStep
        peerId={peerId}
        onSubmit={() => undefined}
        onCreateNew={() => undefined}
      />
    )

    expect(screen.getByText(`Target device: ${formatPeerIdForDisplay(peerId)}`)).toBeInTheDocument()
  })
})
