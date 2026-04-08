import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Button } from '@/components/ui/button'

describe('Button', () => {
  it('suppresses text selection highlight styles on button content', () => {
    render(<Button>创建空间</Button>)

    const button = screen.getByRole('button', { name: '创建空间' })

    expect(button.className).toContain('selection:bg-transparent')
    expect(button.className).toContain('selection:text-current')
  })
})
