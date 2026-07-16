import { render, screen } from '@testing-library/react'
import type { ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import i18n from '@/i18n'

vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: vi.fn(),
}))

vi.mock('@/hooks/useShortcutLayer', () => ({
  useShortcutLayer: vi.fn(),
}))

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

describe('DeleteConfirmDialog i18n', () => {
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

  it('renders translated delete confirmation copy instead of raw keys', () => {
    renderWithI18n(
      <DeleteConfirmDialog
        open
        onOpenChange={() => undefined}
        onConfirm={() => undefined}
        count={1}
      />
    )

    expect(screen.getByText('Confirm Delete')).toBeInTheDocument()
    expect(screen.getByText('Cancel')).toBeInTheDocument()
    expect(screen.getByText('Delete')).toBeInTheDocument()

    expect(screen.queryByText('confirmDeleteTitle')).not.toBeInTheDocument()
    expect(screen.queryByText('cancelLabel')).not.toBeInTheDocument()
    expect(screen.queryByText('deleteLabel')).not.toBeInTheDocument()
  })
})
