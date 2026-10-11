import type { IconName as PublicIconName } from '@/ui-kit/contract'
import type { IconName } from './icons'

/** Maps the public `IconName`s of `@/ui-kit/contract` to the design glyphs. */
export const contractGlyphs = {
  add: 'plus',
  back: 'chevronLeft',
  chevronDown: 'chevronDown',
  chevronRight: 'chevronRight',
  copy: 'copy',
  delete: 'trash',
  desktop: 'desktop',
  devices: 'devices',
  dismiss: 'close',
  edit: 'pencil',
  filter: 'filter',
  history: 'history',
  moon: 'moon',
  more: 'more',
  pin: 'pin',
  search: 'search',
  send: 'arrowRight',
  settings: 'gear',
  sidebar: 'sidebar',
  sun: 'sun',
  tag: 'tag',
} as const satisfies Record<PublicIconName, IconName>

export const glyph = (name: PublicIconName): IconName => contractGlyphs[name]
