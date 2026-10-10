import { THEME_COLORS } from '@/constants/theme'

const PALETTES = new Map(THEME_COLORS.map(option => [option.name, option]))

export default function AppearancePaletteSwatch({ name }: { name: string }) {
  const palette = PALETTES.get(name) ?? THEME_COLORS[0]
  return (
    <span className="flex min-w-0 items-center gap-2.5">
      <span
        aria-hidden="true"
        className="flex h-4 w-7 shrink-0 overflow-hidden rounded-sm border border-border/50"
      >
        {palette.previewDots.slice(0, 3).map((color, index) => (
          <span
            key={`${index}:${color}`}
            className="h-full flex-1"
            style={{ backgroundColor: color }}
          />
        ))}
      </span>
      <span className="truncate capitalize">{palette.name}</span>
    </span>
  )
}
