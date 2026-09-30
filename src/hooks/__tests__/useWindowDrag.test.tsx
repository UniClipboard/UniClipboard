import { cleanup, fireEvent, render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useWindowDrag } from '@/hooks/useWindowDrag'

const mocks = vi.hoisted(() => ({
  platform: { isWindows: false, isMac: false, isLinux: true, isTauri: true },
  startDragging: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/hooks/usePlatform', () => ({ usePlatform: () => mocks.platform }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ startDragging: mocks.startDragging }),
}))

function Surface() {
  const handlers = useWindowDrag()
  return (
    <>
      <div data-testid="surface" {...handlers}>
        <button type="button">control</button>
      </div>
      <div data-testid="outside" />
    </>
  )
}

const pointer = (
  type: 'pointerDown' | 'pointerMove' | 'pointerUp',
  el: Element,
  init: { pointerId?: number; buttons?: number; clientX?: number; clientY?: number } = {}
) =>
  fireEvent[type](el, {
    button: 0,
    pointerId: 1,
    buttons: type === 'pointerUp' ? 0 : 1,
    clientX: 0,
    clientY: 0,
    ...init,
  })

describe('useWindowDrag', () => {
  beforeEach(() => {
    mocks.startDragging.mockClear()
    mocks.platform.isLinux = true
  })
  afterEach(cleanup)

  it('starts a drag once the pressed pointer moves past the threshold', () => {
    const { getByTestId } = render(<Surface />)
    const surface = getByTestId('surface')

    pointer('pointerDown', surface)
    pointer('pointerMove', surface, { clientX: 2 })
    expect(mocks.startDragging).not.toHaveBeenCalled()
    pointer('pointerMove', surface, { clientX: 10 })
    pointer('pointerMove', surface, { clientX: 20 })

    expect(mocks.startDragging).toHaveBeenCalledTimes(1)
  })

  it('ignores presses on interactive children', () => {
    const { getByTestId, getByText } = render(<Surface />)

    pointer('pointerDown', getByText('control'))
    pointer('pointerMove', getByTestId('surface'), { clientX: 20 })

    expect(mocks.startDragging).not.toHaveBeenCalled()
  })

  it('drops a press released outside the surface', () => {
    const { getByTestId } = render(<Surface />)
    const surface = getByTestId('surface')
    const outside = getByTestId('outside')

    pointer('pointerDown', surface)
    pointer('pointerUp', outside)
    // A later press that starts outside and moves into the surface must not drag.
    pointer('pointerDown', outside, { clientX: 500 })
    pointer('pointerMove', surface, { clientX: 20 })

    expect(mocks.startDragging).not.toHaveBeenCalled()
  })

  it('drops a press when the window loses focus', () => {
    const { getByTestId } = render(<Surface />)
    const surface = getByTestId('surface')

    pointer('pointerDown', surface)
    fireEvent.blur(window)
    pointer('pointerMove', surface, { clientX: 20 })

    expect(mocks.startDragging).not.toHaveBeenCalled()
  })

  it('ignores movement from a different pointer', () => {
    const { getByTestId } = render(<Surface />)
    const surface = getByTestId('surface')

    pointer('pointerDown', surface, { pointerId: 1 })
    pointer('pointerMove', surface, { pointerId: 2, clientX: 20 })
    expect(mocks.startDragging).not.toHaveBeenCalled()

    pointer('pointerMove', surface, { pointerId: 1, clientX: 20 })
    expect(mocks.startDragging).toHaveBeenCalledTimes(1)
  })

  it('does not leave window listeners behind after unmount', () => {
    const remove = vi.spyOn(window, 'removeEventListener')
    const { getByTestId, unmount } = render(<Surface />)

    pointer('pointerDown', getByTestId('surface'))
    unmount()

    expect(remove).toHaveBeenCalledWith('pointerup', expect.any(Function), true)
    expect(remove).toHaveBeenCalledWith('pointercancel', expect.any(Function), true)
    expect(remove).toHaveBeenCalledWith('blur', expect.any(Function))
    remove.mockRestore()
  })

  it('does nothing outside Linux', () => {
    mocks.platform.isLinux = false
    const { getByTestId } = render(<Surface />)
    const surface = getByTestId('surface')

    pointer('pointerDown', surface)
    pointer('pointerMove', surface, { clientX: 20 })

    expect(mocks.startDragging).not.toHaveBeenCalled()
  })
})
