import type { ComponentProps, ReactNode } from 'react'
import { deliveries, tagColor, type Clip, type SidebarItem } from '@/prototypes/macos-design/data'
import { Icon, type IconName } from '@/ui-kit/skins/macos/icons'
import { Checkbox, PushButton, Tag } from '@/ui-kit/skins/macos/parts'

const thumbLabel = { code: '</>', url: 'URL', text: 'TXT', pdf: 'PDF', image: '' } as const

export function ClipRow({
  clip,
  selected = false,
  multiSelect,
  ...props
}: {
  clip: Clip
  selected?: boolean
  /** Shows the multi-select checkbox; `true` = checked. */
  multiSelect?: boolean
} & ComponentProps<'div'>) {
  return (
    <div className="mac-row" role="option" aria-selected={selected} {...props}>
      {multiSelect !== undefined && (
        <Checkbox
          checked={multiSelect}
          variant={multiSelect ? 'on-selection' : 'default'}
          tabIndex={-1}
          aria-label={`Select ${clip.title}`}
        />
      )}
      <span className="mac-thumb" data-kind={clip.kind} aria-hidden="true">
        {thumbLabel[clip.kind]}
      </span>
      <span className="mac-row-text">
        <span className="mac-row-title" data-mono={clip.kind === 'code' ? '' : undefined}>
          {clip.title}
        </span>
        <span className="mac-row-meta">
          {clip.source} · {clip.time}
        </span>
      </span>
      <span className="mac-row-trailing">
        {clip.pinned && (
          <span className="mac-row-pin" aria-label="Pinned">
            <Icon name="pin" size={12} strokeWidth={1.5} />
          </span>
        )}
        {clip.tags.map(tag => (
          <Tag key={tag} label={`#${tag}`} color={tagColor[tag]} />
        ))}
      </span>
    </div>
  )
}

const smartColor = { orange: 'var(--orange)', green: 'var(--green)' }
const dotColor = { blue: 'var(--blue)', orange: 'var(--orange)' }

export function SidebarRow({
  item,
  selected = false,
  ...props
}: { item: SidebarItem; selected?: boolean } & ComponentProps<'div'>) {
  return (
    <div className="mac-side-item" role="option" aria-selected={selected} {...props}>
      <span className="mac-side-icon">
        {'icon' in item && <Icon name={item.icon as IconName} size={15} />}
        {'smart' in item && (
          <span className="mac-dot-smart" style={{ background: smartColor[item.smart] }} />
        )}
        {'tagDot' in item && (
          <span className="mac-dot-tag" style={{ background: dotColor[item.tagDot] }} />
        )}
      </span>
      <span className="mac-side-label">{item.label}</span>
      <span className="mac-side-count">{item.count}</span>
    </div>
  )
}

export function CodeCard({ body, lines, chars }: { body: string; lines: number; chars: number }) {
  return (
    <figure className="mac-code" style={{ margin: 0 }}>
      <figcaption className="mac-code-header">
        <span>
          Shell · {lines} lines · {chars} chars
        </span>
        <kbd>⌘C</kbd>
      </figcaption>
      <pre>{body}</pre>
    </figure>
  )
}

const toneColor = {
  green: 'var(--green)',
  orange: 'var(--orange)',
  gray: 'var(--gray)',
  red: 'var(--red)',
}

export function DeliveryList() {
  return (
    <ul className="mac-grouped" aria-label="Delivery">
      {deliveries.map(row => (
        <li key={row.device}>
          <span className="mac-status-dot" style={{ background: toneColor[row.tone] }} />
          <span>{row.device}</span>
          <span
            className="mac-status"
            style={{ color: row.tone === 'gray' ? 'var(--label-2)' : toneColor[row.tone] }}
          >
            {row.status}
          </span>
        </li>
      ))}
    </ul>
  )
}

const noticeIcon = {
  blue: 'info',
  green: 'checkCircle',
  orange: 'warning',
  red: 'xCircle',
} as const

export function InlineNotice({
  tone,
  title,
  text,
  action,
}: {
  tone: keyof typeof noticeIcon
  title: string
  text: string
  action?: ReactNode
}) {
  return (
    <div className="mac-notice" data-tone={tone} role={tone === 'red' ? 'alert' : 'status'}>
      <Icon name={noticeIcon[tone]} size={16} color={`var(--${tone})`} />
      <span className="mac-notice-title">{title}</span>
      <span className="mac-notice-text">{text}</span>
      {action && <span className="mac-notice-action">{action}</span>}
    </div>
  )
}

export function ActionBar({ onCopy, sendTo }: { onCopy?: () => void; sendTo?: ReactNode }) {
  return (
    <div className="mac-actionbar mac-glass" role="toolbar" aria-label="Item actions">
      <PushButton size="xlarge" tint="primary" icon="copy" onClick={onCopy}>
        Copy
      </PushButton>
      {sendTo ?? (
        <PushButton size="large" tint="glass" icon="arrowRight">
          Send to…
        </PushButton>
      )}
      <span className="mac-actionbar-hint">E edit · T tag · ⌘P pin</span>
    </div>
  )
}

export function NotificationCard() {
  return (
    <div className="mac-notification mac-glass">
      <span className="mac-notification-icon" />
      <div className="mac-notification-body">
        <span className="mac-notification-title">
          From iPhone 16<span className="mac-notification-time">now</span>
        </span>
        <span className="mac-notification-text">
          “Meet at Gate 14, boarding 18:40” — in your clipboard. Press ⌘V.
        </span>
      </div>
    </div>
  )
}
