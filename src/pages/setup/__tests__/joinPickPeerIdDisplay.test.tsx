import { render, screen } from '@testing-library/react'
import type { ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import i18n from '@/i18n'
import { formatPeerIdForDisplay } from '@/lib/utils'
import JoinPickDeviceStep from '@/pages/setup/JoinPickDeviceStep'

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

describe('setup join pick peer id display', () => {
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

  it('JoinPickDeviceStep uses formatPeerIdForDisplay', () => {
    const peerId = '12D3KooWABCDEFGH'
    renderWithI18n(
      <JoinPickDeviceStep
        onSelectPeer={() => undefined}
        onBack={() => undefined}
        onRescan={() => undefined}
        peers={[{ id: peerId, deviceName: 'Device A', device_type: 'desktop' }]}
        scanPhase="hasDevices"
      />
    )

    expect(screen.getByText(formatPeerIdForDisplay(peerId))).toBeInTheDocument()
  })
})
