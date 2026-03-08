import { readFileSync } from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'
import { describe, expect, it } from 'vitest'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const globals = readFileSync(path.resolve(__dirname, '../../styles/globals.css'), 'utf8')

describe('theme migration', () => {
  it('does not import legacy theme CSS files', () => {
    expect(globals).not.toContain("@import './themes/")
  })

  it('has a transition rule for color-driven surfaces', () => {
    expect(globals).toContain('transition')
    expect(globals).toContain('background-color 200ms')
  })
})
