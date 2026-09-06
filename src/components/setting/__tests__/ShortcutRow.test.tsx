import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ShortcutRow } from '@/components/setting/ShortcutRow'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { SHORTCUT_DEFINITIONS } from '@/shortcuts/definitions'

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
const definition = SHORTCUT_DEFINITIONS.find(item => item.id === 'nav.settings')!
function setup() {
  const save = vi.fn()
  render(
    <ShortcutProvider>
      <ShortcutRow
        definition={definition}
        currentKey="meta+,"
        currentOverrides={{}}
        isModified={false}
        onOverrideChange={save}
        onResetShortcut={vi.fn()}
      />
    </ShortcutProvider>
  )
  return save
}

describe('shortcut recorder popover', () => {
  it('keeps the row visible and cancels without saving', async () => {
    const user = userEvent.setup()
    const save = setup()
    const label = screen.getByText(definition.description)
    const trigger = screen.getByRole('button', { name: /settings.sections.shortcuts.edit/ })
    await user.click(trigger)
    await screen.findByRole('dialog')
    expect(label).toBeInTheDocument()
    await user.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(save).not.toHaveBeenCalled()
    await waitFor(() => expect(trigger).toHaveFocus())
  })

  it('records two combinations, ignores repeats, and saves only on confirmation', async () => {
    const user = userEvent.setup()
    const save = setup()
    await user.click(screen.getByRole('button', { name: /settings.sections.shortcuts.edit/ }))
    const field = await screen.findByRole('group', {
      name: 'settings.sections.shortcuts.recording',
    })
    field.focus()
    fireEvent.keyDown(field, { key: 'k', metaKey: true })
    fireEvent.keyDown(field, { key: 'k', metaKey: true, repeat: true })
    fireEvent.keyDown(field, { key: 'c', metaKey: true })
    expect(save).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'settings.sections.shortcuts.save' }))
    expect(save).toHaveBeenCalledWith('nav.settings', 'meta+k meta+c', undefined)
  })

  it('lets Tab reach action buttons without adding another recorded key', async () => {
    const user = userEvent.setup()
    const save = setup()
    await user.click(screen.getByRole('button', { name: /settings.sections.shortcuts.edit/ }))
    const field = await screen.findByRole('group', {
      name: 'settings.sections.shortcuts.recording',
    })
    field.focus()
    fireEvent.keyDown(field, { key: 'j', metaKey: true })
    await user.tab()
    expect(field).not.toHaveFocus()
    const button = screen.getByRole('button', { name: 'settings.sections.shortcuts.save' })
    button.focus()
    await user.keyboard('{Enter}')
    expect(save).toHaveBeenCalledWith('nav.settings', 'meta+j', undefined)
  })

  it('discards the draft on outside click and starts fresh when reopened', async () => {
    const user = userEvent.setup()
    const save = setup()
    const trigger = screen.getByRole('button', { name: /settings.sections.shortcuts.edit/ })
    await user.click(trigger)
    const field = await screen.findByRole('group', {
      name: 'settings.sections.shortcuts.recording',
    })
    fireEvent.keyDown(field, { key: 'j', metaKey: true })
    await user.click(document.body)
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    await user.click(trigger)
    expect(
      await screen.findByRole('button', { name: 'settings.sections.shortcuts.save' })
    ).toBeDisabled()
    expect(save).not.toHaveBeenCalled()
  })

  it('requires explicit confirmation before clearing a conflicting binding', async () => {
    const user = userEvent.setup()
    const save = vi.fn()
    render(
      <ShortcutProvider>
        <ShortcutRow
          definition={definition}
          currentKey="meta+comma"
          currentOverrides={{ 'global.zoomIn': 'meta+j' }}
          isModified={false}
          onOverrideChange={save}
          onResetShortcut={vi.fn()}
        />
      </ShortcutProvider>
    )
    const trigger = screen.getByRole('button', { name: /settings.sections.shortcuts.edit/ })
    expect(trigger).toHaveTextContent(',')
    expect(trigger).not.toHaveTextContent('Comma')
    await user.click(trigger)
    const field = await screen.findByRole('group', {
      name: 'settings.sections.shortcuts.recording',
    })
    fireEvent.keyDown(field, { key: 'j', metaKey: true })
    expect(screen.getByRole('alert')).toBeInTheDocument()
    expect(save).not.toHaveBeenCalled()
    await user.click(
      screen.getByRole('button', { name: 'settings.sections.shortcuts.confirmOverride' })
    )
    expect(save).toHaveBeenCalledWith('nav.settings', 'meta+j', ['global.zoomIn'])
  })
})
