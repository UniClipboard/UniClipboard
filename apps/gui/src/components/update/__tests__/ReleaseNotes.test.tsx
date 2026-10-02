import { openUrl } from '@tauri-apps/plugin-opener'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ i18n: { language: 'en' } }),
}))

describe('ReleaseNotes', () => {
  it('renders headings, lists, and explicit links without optional Markdown extensions', async () => {
    const user = userEvent.setup()
    render(
      <ReleaseNotes
        content={'## Changes\n\n- Fixed startup\n\n[Read more](https://example.com/release)'}
        fallback="No notes"
      />
    )

    expect(screen.getByRole('heading', { name: 'Changes' })).toBeInTheDocument()
    expect(screen.getByRole('listitem')).toHaveTextContent('Fixed startup')

    await user.click(screen.getByRole('link', { name: 'Read more' }))

    expect(openUrl).toHaveBeenCalledWith('https://example.com/release')
  })
})
