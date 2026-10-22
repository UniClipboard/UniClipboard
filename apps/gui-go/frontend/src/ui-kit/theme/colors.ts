import { converter, formatRgb, parse } from 'culori'

const rgb = converter('rgb')

/** Precompute state colours for WebKit versions without color-mix(). */
export function alpha(color: string, opacity: number): string {
  const value = rgb(parse(color))
  return value ? formatRgb({ ...value, alpha: opacity }) : color
}

export function mix(color: string, other: string, weight: number): string {
  const a = rgb(parse(color))
  const b = rgb(parse(other))
  if (!a || !b) return color
  return formatRgb({
    mode: 'rgb',
    r: a.r * (1 - weight) + b.r * weight,
    g: a.g * (1 - weight) + b.g * weight,
    b: a.b * (1 - weight) + b.b * weight,
  })
}

export function normalizeColors<T extends Record<string, string>>(colors: T): T {
  return Object.fromEntries(
    Object.entries(colors).map(([key, value]) => {
      const parsed = parse(value)
      return [key, parsed ? formatRgb(rgb(parsed)) : value]
    })
  ) as T
}
