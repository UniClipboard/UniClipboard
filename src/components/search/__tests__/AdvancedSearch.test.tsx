import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import AdvancedSearch from '../AdvancedSearch'

describe('AdvancedSearch', () => {
  it('enters advanced mode when full-width colon is entered', () => {
    const onAdvancedChange = vi.fn()
    const onValueChange = vi.fn()

    render(
      <AdvancedSearch
        value=""
        onValueChange={onValueChange}
        isAdvanced={false}
        onAdvancedChange={onAdvancedChange}
        tokens={[]}
        onTokensChange={vi.fn()}
      />
    )

    const input = screen.getByRole('textbox')

    fireEvent.change(input, { target: { value: '：' } })

    expect(onAdvancedChange).toHaveBeenCalledWith(true)
    expect(onValueChange).toHaveBeenCalledWith('')
  })

  it('does not emit intermediate IME text until composition completes', () => {
    const onValueChange = vi.fn()

    function Harness() {
      const [value, setValue] = useState('')

      return (
        <AdvancedSearch
          value={value}
          onValueChange={nextValue => {
            onValueChange(nextValue)
            setValue(nextValue)
          }}
          isAdvanced={false}
          onAdvancedChange={vi.fn()}
          tokens={[]}
          onTokensChange={vi.fn()}
        />
      )
    }

    render(<Harness />)

    const input = screen.getByRole('textbox')

    fireEvent.compositionStart(input)
    fireEvent.change(input, { target: { value: 'zhong' } })

    expect(onValueChange).not.toHaveBeenCalled()

    fireEvent.compositionEnd(input, { data: '中', target: { value: '中' } })

    expect(onValueChange).toHaveBeenCalledWith('中')
    expect(onValueChange).toHaveBeenCalledTimes(1)
  })

  it('does not forward enter while IME composition is active', () => {
    const onKeyDown = vi.fn()

    render(
      <AdvancedSearch
        value=""
        onValueChange={vi.fn()}
        isAdvanced={false}
        onAdvancedChange={vi.fn()}
        tokens={[]}
        onTokensChange={vi.fn()}
        onKeyDown={onKeyDown}
      />
    )

    const input = screen.getByRole('textbox')

    fireEvent.compositionStart(input)
    fireEvent.keyDown(input, { key: 'Enter', isComposing: true })

    expect(onKeyDown).not.toHaveBeenCalled()
  })

  it('does not forward the enter used immediately after composition commits', () => {
    const onKeyDown = vi.fn()

    render(
      <AdvancedSearch
        value=""
        onValueChange={vi.fn()}
        isAdvanced={false}
        onAdvancedChange={vi.fn()}
        tokens={[]}
        onTokensChange={vi.fn()}
        onKeyDown={onKeyDown}
      />
    )

    const input = screen.getByRole('textbox')

    fireEvent.compositionStart(input)
    fireEvent.compositionEnd(input, { data: '中', target: { value: '中' } })
    fireEvent.keyDown(input, { key: 'Enter' })

    expect(onKeyDown).not.toHaveBeenCalled()
  })
})
