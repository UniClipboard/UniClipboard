import { describe, expect, it } from 'vitest'
import { parseTokens } from '../useHistorySearch'

describe('parseTokens', () => {
  it('parses # tokens as tags', () => {
    expect(parseTokens(['#link', '#code'])).toMatchObject({
      contentTypes: [],
      tags: ['link', 'code'],
    })
  })

  it('keeps supported type tokens limited to physical content types', () => {
    expect(parseTokens(['type:text', 'type:file'])).toMatchObject({
      contentTypes: ['text', 'file'],
      tags: [],
    })
  })
})
