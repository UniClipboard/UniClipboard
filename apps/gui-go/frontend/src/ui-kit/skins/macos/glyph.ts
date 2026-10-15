import type { IconName as PublicIconName } from '@/ui-kit/contract'
import type { IconName } from './icons'

/** Maps the public `IconName`s of `@/ui-kit/contract` to the design glyphs. */
export const contractGlyphs = {
  add: 'plus',
  back: 'chevronLeft',
  chat: 'message',
  chevronDown: 'chevronDown',
  chevronRight: 'chevronRight',
  copy: 'copy',
  delete: 'trash',
  desktop: 'desktop',
  devices: 'devices',
  dismiss: 'close',
  download: 'download',
  edit: 'pencil',
  file: 'doc',
  filter: 'filter',
  globe: 'globe',
  grid: 'grid',
  history: 'history',
  info: 'info',
  list: 'list',
  lock: 'lock',
  moon: 'moon',
  more: 'more',
  package: 'box',
  phone: 'phone',
  pin: 'pin',
  remove: 'minus',
  retry: 'retry',
  search: 'search',
  send: 'arrowRight',
  settings: 'gear',
  shield: 'shield',
  sidebar: 'sidebar',
  sun: 'sun',
  tag: 'tag',
  unlock: 'lockOpen',
  update: 'arrowUpCircle',
} as const satisfies Record<PublicIconName, IconName>

export const glyph = (name: PublicIconName): IconName => contractGlyphs[name]
