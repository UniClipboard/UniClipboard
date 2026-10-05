import type { Dimension } from './composite-search-model'

/**
 * Per-dimension color of the list variant (HList.dc.html): the chip in the
 * field, the token being typed and the suggestion values share it, so a filter
 * reads as the same kind of thing at every step. Sources are green, tags
 * orange, time inverted; type and extension, which the design leaves out,
 * take blue and neutral.
 */
export const DIMENSION_CHIP_CLASS: Record<Dimension, string> = {
  type: 'bg-blue-500/10 text-blue-800 dark:text-blue-300',
  tag: 'bg-orange-500/12 text-orange-800 dark:text-orange-300',
  source: 'bg-emerald-500/12 text-emerald-800 dark:text-emerald-300',
  time: 'bg-foreground text-background',
  extension: 'bg-muted text-foreground/75',
}

/** Text color of a `key:value` token while it is typed. */
export const DIMENSION_INK_CLASS: Record<Dimension, string> = {
  type: 'text-blue-800 dark:text-blue-300',
  tag: 'text-orange-800 dark:text-orange-300',
  source: 'text-emerald-800 dark:text-emerald-300',
  time: 'text-foreground',
  extension: 'text-foreground/75',
}

/** The syntax word a chip names its filter by ("from arch-desktop"). Fixed
 * English like the typed keys; the tag dimension, typed as `#`, reads `tag`. */
export const DIMENSION_CHIP_KEY: Record<Dimension, string> = {
  type: 'type',
  tag: 'tag',
  source: 'from',
  time: 'on',
  extension: 'ext',
}
