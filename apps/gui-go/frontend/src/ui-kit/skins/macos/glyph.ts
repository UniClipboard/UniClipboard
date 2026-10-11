import type { IconName as PublicIconName } from '@/ui-kit/contract'
import { contractGlyphs, type IconName } from './icons'

// A new public icon fails here until the macOS skin maps it to a glyph.
const glyphs: Record<PublicIconName, IconName> = contractGlyphs

export const glyph = (name: PublicIconName): IconName => glyphs[name]
