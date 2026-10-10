import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { HistoryTagDto } from '@/api/daemon/history-tags'
import HistoryTagEditor from '../HistoryTagEditor'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

const release: HistoryTagDto = {
  tagId: 't-release',
  name: 'release',
  entryCount: 12,
  createdAtMs: 0,
}

function renderEditor(onPick = vi.fn().mockResolvedValue(true)) {
  render(
    <HistoryTagEditor tags={[release]} attachedIds={new Set()} onPick={onPick} onClose={vi.fn()} />
  )
  return onPick
}

describe('HistoryTagEditor', () => {
  it('creates a new tag in the first color by default', async () => {
    const user = userEvent.setup()
    const onPick = renderEditor()

    await user.keyboard('hotfix{Enter}')
    expect(onPick).toHaveBeenCalledWith({ kind: 'create', name: 'hotfix' }, 'orange')
  })

  it('cycles the new tag color with Tab and Shift+Tab', async () => {
    const user = userEvent.setup()
    const onPick = renderEditor()

    await user.keyboard('hotfix{Tab}{Tab}')
    expect(screen.getByRole('radio', { name: 'history.tags.colors.green' })).toHaveAttribute(
      'aria-checked',
      'true'
    )
    await user.keyboard('{Shift>}{Tab}{/Shift}{Enter}')
    expect(onPick).toHaveBeenCalledWith({ kind: 'create', name: 'hotfix' }, 'blue')
  })

  it('picks a color with the pointer', async () => {
    const user = userEvent.setup()
    const onPick = renderEditor()

    await user.keyboard('hotfix')
    await user.click(screen.getByRole('radio', { name: 'history.tags.colors.purple' }))
    await user.keyboard('{Enter}')
    expect(onPick).toHaveBeenCalledWith({ kind: 'create', name: 'hotfix' }, 'purple')
  })

  it('creates a tag in a custom color picked from the color area', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()
    const onPick = vi.fn().mockResolvedValue(true)
    render(
      <HistoryTagEditor
        tags={[release]}
        attachedIds={new Set()}
        onPick={onPick}
        onClose={onClose}
      />
    )

    await user.keyboard('hotfix')
    await user.click(screen.getByRole('radio', { name: 'history.tags.customColor' }))
    const hue = await screen.findByRole('slider', { name: 'Hue' })
    // react-colorful reads the legacy keyCode.
    fireEvent.keyDown(hue, { key: 'ArrowRight', keyCode: 39, which: 39 })
    // Working in the picker does not close the editor.
    expect(onClose).not.toHaveBeenCalled()
    const custom = screen.getByRole('radio', { name: 'history.tags.customColor' })
    expect(custom).toHaveAttribute('aria-checked', 'true')

    await user.click(screen.getByRole('button', { name: /history.tags.create/ }))
    expect(onPick).toHaveBeenCalledWith(
      { kind: 'create', name: 'hotfix' },
      expect.stringMatching(/^#[0-9a-f]{6}$/)
    )
  })
})
