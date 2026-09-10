import { act, render, screen, waitFor } from '@testing-library/react'
import { createRef } from 'react'
import { Virtuoso, VirtuosoMockContext, type VirtuosoHandle } from 'react-virtuoso'
import { describe, expect, it, vi } from 'vitest'
import { HistoryScroller, HistoryList } from '@/components/history/history-scroll-components'

const historyScrollComponents = { Scroller: HistoryScroller, List: HistoryList }

describe('history overlay scrolling', () => {
  it('keeps virtualized navigation attached to the actual scrolling viewport', async () => {
    const ref = createRef<VirtuosoHandle>()
    let scroller: HTMLElement | null = null
    render(
      <VirtuosoMockContext value={{ viewportHeight: 400, itemHeight: 80 }}>
        <Virtuoso
          ref={ref}
          components={historyScrollComponents}
          totalCount={100}
          scrollerRef={element => {
            scroller = element as HTMLElement | null
          }}
          itemContent={index => <div>Entry {index}</div>}
        />
      </VirtuosoMockContext>
    )
    await screen.findByText('Entry 0')
    expect(screen.queryByText('Entry 99')).not.toBeInTheDocument()
    expect(scroller).not.toBeNull()
    const viewport = scroller as unknown as HTMLElement
    const content = viewport.querySelector('[role="presentation"]')
    expect(content).not.toBeNull()
    expect(content).toHaveStyle({ width: '100%', minWidth: '0px' })
    Object.defineProperties(viewport, {
      scrollHeight: { value: 8000 },
      offsetHeight: { value: 400 },
    })
    const scrollTo = vi.fn()
    viewport.scrollTo = scrollTo
    act(() => ref.current?.scrollToIndex({ index: 70, align: 'start' }))
    await waitFor(() =>
      expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ top: 5600 }))
    )
  })
})
