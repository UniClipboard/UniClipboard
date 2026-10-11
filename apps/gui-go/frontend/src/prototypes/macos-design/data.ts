// Synthetic content taken from the design canvas copy. No production data is read.
import type { TagPaletteColor } from '@/ui-kit/contract'

export type TagName = 'deploy' | 'docker' | 'git' | 'design' | 'finance' | 'work'

/** The palette colour of each sample tag (design T1 tints). */
export const tagColor = {
  deploy: 'orange',
  docker: 'blue',
  git: 'gray',
  design: 'purple',
  finance: 'green',
  work: 'blue',
} as const satisfies Record<TagName, TagPaletteColor>

export type ClipKind = 'code' | 'url' | 'text' | 'pdf' | 'image'

export interface Clip {
  id: string
  kind: ClipKind
  title: string
  source: string
  time: string
  tags: TagName[]
  pinned?: boolean
  body?: string
}

export const clips: Clip[] = [
  {
    id: 'rebase',
    kind: 'code',
    title: 'git rebase -i HEAD~3',
    source: 'arch-desktop · Alacritty',
    time: '09:38:12',
    tags: ['git'],
    body: 'git rebase -i HEAD~3',
  },
  {
    id: 'logs',
    kind: 'code',
    title: 'docker compose logs -f api',
    source: 'arch-desktop · Alacritty',
    time: '09:38:12',
    tags: ['git'],
    body: 'docker compose logs -f api',
  },
  {
    id: 'compose-up',
    kind: 'code',
    title: 'docker compose -f compose.prod.yml up -d',
    source: 'arch-desktop · Alacritty',
    time: '22:14:08',
    tags: ['deploy', 'docker'],
    pinned: true,
    body: 'docker compose -f compose.prod.yml \\\n  up -d --build --remove-orphans',
  },
  {
    id: 'repo',
    kind: 'url',
    title: 'https://github.com/UniClipboard/UniClipboard',
    source: 'iPhone 16 · Safari',
    time: '09:37:40',
    tags: [],
  },
  {
    id: 'roadmap',
    kind: 'text',
    title: 'Q4 roadmap — ship LAN-only Mode, then per-device gates',
    source: 'Windows PC · Obsidian',
    time: '09:26:51',
    tags: ['work'],
  },
  {
    id: 'invoice',
    kind: 'pdf',
    title: 'invoice-0921.pdf',
    source: 'Pixel 8 · Files',
    time: '08:40:19',
    tags: ['finance'],
  },
  {
    id: 'screenshot',
    kind: 'image',
    title: 'Screenshot 2026-09-28 at 09.34.02.png',
    source: 'MacBook Pro · Screenshot',
    time: '09:34:02',
    tags: ['design'],
  },
]

export type SidebarItem =
  | { id: string; label: string; count: string; icon: 'stack' | 'pin' | 'trash' }
  | { id: string; label: string; count: string; smart: 'orange' | 'green' }
  | { id: string; label: string; count: string; tagDot: 'blue' | 'orange' }

export const sidebarSections: { title?: string; items: SidebarItem[] }[] = [
  {
    items: [
      { id: 'all', label: 'All items', count: '12,408', icon: 'stack' },
      { id: 'pinned', label: 'Pinned', count: '24', icon: 'pin' },
      { id: 'deleted', label: 'Recently deleted', count: '12', icon: 'trash' },
    ],
  },
  {
    title: 'Smart Views',
    items: [
      { id: 'deploy-commands', label: 'Deploy commands', count: '14', smart: 'orange' },
      { id: 'invoices', label: 'Invoices', count: '9', smart: 'green' },
    ],
  },
  {
    title: 'Tags',
    items: [
      { id: 'tag-docker', label: '#docker', count: '38', tagDot: 'blue' },
      { id: 'tag-deploy', label: '#deploy', count: '14', tagDot: 'orange' },
    ],
  },
]

export type DeliveryTone = 'green' | 'orange' | 'gray' | 'red'

export const deliveries: { device: string; status: string; tone: DeliveryTone }[] = [
  { device: 'MacBook Pro', status: 'In clipboard · 0.3 s', tone: 'green' },
  { device: 'iPhone 16', status: 'Relay · 62%', tone: 'orange' },
  { device: 'Pixel 8', status: 'Waiting — offline', tone: 'gray' },
  { device: 'arch-desktop', status: 'Blocked by its rules', tone: 'red' },
]

export const sendTargets = [
  { id: 'iphone', name: 'iPhone 16', route: 'direct', defaultChecked: true },
  { id: 'windows', name: 'Windows PC', route: 'relay', defaultChecked: true },
  { id: 'pixel', name: 'Pixel 8', route: 'offline', defaultChecked: false },
]
