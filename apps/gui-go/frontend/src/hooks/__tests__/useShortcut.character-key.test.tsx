import { fireEvent, render, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'

function CharacterShortcut({ onOpen }: { onOpen: () => void }) {
  useShortcutScope('clipboard')
  useShortcut({
    key: ['/', '、'],
    scope: 'clipboard',
    handler: onOpen,
    useKey: true,
  })
  return null
}

describe('useShortcut character matching', () => {
  it.each([
    ['/', 'Slash'],
    ['、', 'Backslash'],
  ])('matches the typed %s character regardless of physical code', async (key, code) => {
    const onOpen = vi.fn()
    render(
      <ShortcutProvider>
        <CharacterShortcut onOpen={onOpen} />
      </ShortcutProvider>
    )

    await waitFor(() => {
      fireEvent.keyDown(document, { key, code })
      expect(onOpen).toHaveBeenCalledTimes(1)
    })
    fireEvent.keyUp(document, { key, code })
  })
})
