import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { type LibraryChrome, LibraryChromeContext } from '@/contexts/library-chrome-context'
import TrafficLightOverhang from '../TrafficLightOverhang'

vi.mock('@/hooks/useWindowDragging', () => ({ useWindowDragging: () => ({}) }))
const platform = vi.hoisted(() => ({ isMac: true }))
vi.mock('@/hooks/usePlatform', () => ({ usePlatform: () => ({ isMac: platform.isMac }) }))

function renderOverhang(isMac: boolean, hidden: boolean) {
  platform.isMac = isMac
  const chrome: LibraryChrome = {
    hidden,
    drawer: false,
    drawerOpen: false,
    toggle: vi.fn(),
    closeDrawer: vi.fn(),
    setLightsInContent: vi.fn(),
  }
  return render(
    <LibraryChromeContext value={chrome}>
      <TrafficLightOverhang className="w-3.5" />
    </LibraryChromeContext>
  )
}

describe('TrafficLightOverhang', () => {
  it('reserves a drag strip under the lights while the macOS sidebar is collapsed', () => {
    const { container } = renderOverhang(true, true)

    expect(container.firstElementChild).toHaveAttribute('data-tauri-drag-region')
  })

  it('renders nothing while the sidebar is shown or off macOS', () => {
    expect(renderOverhang(true, false).container.firstElementChild).toBeNull()
    expect(renderOverhang(false, true).container.firstElementChild).toBeNull()
  })
})
