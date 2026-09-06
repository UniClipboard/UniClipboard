import { formatShortcutChord, isMac } from '@/lib/shortcut-format'

export function ShortcutKeys({ shortcut }: { shortcut: string }) {
  const label = formatShortcutChord(shortcut)
    .map(parts => parts.join(isMac ? '' : '+'))
    .join(' › ')

  return <kbd className="font-sans text-sm font-medium tracking-wide">{label}</kbd>
}
