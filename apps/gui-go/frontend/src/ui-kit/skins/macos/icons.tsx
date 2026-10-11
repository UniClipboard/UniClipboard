// Line icons copied from the design canvas (boards T2 and C1–C5). macOS draws SF Symbols, which only ship on Apple platforms, so the
// design substitutes same-style strokes; the paths are kept verbatim so screenshots compare like for like.
const paths = {
  copy: (
    <>
      <rect x="8" y="8" width="12" height="12" rx="2" />
      <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
    </>
  ),
  arrowRight: <path d="M4 12h14M13 6l6 6-6 6" />,
  pin: <path d="M12 17v5M8 3h8l-1 6 3 4H6l3-4z" />,
  stack: (
    <>
      <rect x="7" y="3" width="13" height="15" rx="3" />
      <path d="M4 7.5v10A3.5 3.5 0 0 0 7.5 21H15" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </>
  ),
  check: <path d="m5 12 5 5 9-10" />,
  trash: <path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />,
  sidebar: <path d="M4 6h16M4 12h16M4 18h16" />,
  more: (
    <>
      <circle cx="5" cy="12" r=".9" />
      <circle cx="12" cy="12" r=".9" />
      <circle cx="19" cy="12" r=".9" />
    </>
  ),
  chevronLeft: <path d="m15 6-6 6 6 6" />,
  chevronDown: <path d="m6 9 6 6 6-6" />,
  close: <path d="M6 6l12 12M18 6 6 18" />,
  info: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5M12 7.5v.5" />
    </>
  ),
  checkCircle: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="m8 12 3 3 5-6" />
    </>
  ),
  warning: (
    <>
      <path d="M12 3 2 20h20z" />
      <path d="M12 10v4M12 17v.5" />
    </>
  ),
  xCircle: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M9 9l6 6M15 9l-6 6" />
    </>
  ),
  pencil: <path d="M4 20h4L19 9l-4-4L4 16z" />,
  tag: (
    <>
      <path d="M3 12V4h8l10 10-8 8z" />
      <circle cx="7.5" cy="8.5" r="1.3" />
    </>
  ),
  grid: (
    <>
      <rect x="7" y="3" width="13" height="15" rx="3" />
      <path d="M4 7.5v10A3.5 3.5 0 0 0 7.5 21H15" />
    </>
  ),
  list: <path d="M4 6h16M4 12h16M4 18h16" />,
  chevronRight: <path d="m9 6 6 6-6 6" />,
  plus: <path d="M12 5v14M5 12h14" />,
  filter: <path d="M4 5h16l-6 7.5V19l-4 2v-8.5z" />,
  devices: (
    <>
      <rect x="2" y="4" width="14" height="10" rx="1.5" />
      <path d="M6 18h6M9 14v4" />
      <rect x="17" y="8" width="5" height="12" rx="1.2" />
    </>
  ),
  gear: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 2.8v2.4M12 18.8v2.4M4.4 4.4l1.7 1.7M17.9 17.9l1.7 1.7M2.8 12h2.4M18.8 12h2.4M4.4 19.6l1.7-1.7M17.9 6.1l1.7-1.7" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </>
  ),
  moon: <path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z" />,
  desktop: (
    <>
      <rect x="3" y="4" width="18" height="13" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </>
  ),
  // Not in the design canvas; drawn in the same stroke style.
  history: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
} as const

export type IconName = keyof typeof paths

export function Icon({
  name,
  size = 16,
  strokeWidth = 1.8,
  color = 'currentColor',
}: {
  name: IconName
  size?: number
  strokeWidth?: number
  color?: string
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className="mac-icon"
    >
      {paths[name]}
    </svg>
  )
}

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
} as const satisfies Record<string, IconName>
