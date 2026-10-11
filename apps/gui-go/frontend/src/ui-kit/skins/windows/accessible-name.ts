// The accessible name always comes from aria-label (or visible text). A tooltip that only repeats
// the name is visual (`inaccessible`), so screen readers do not announce it twice; a tooltip with
// different text is a description.
export function nameProps(label?: string, tooltip?: string) {
  const content = tooltip ?? label
  return {
    ariaLabel: label,
    tooltip: content,
    relationship: content === label ? ('inaccessible' as const) : ('description' as const),
  }
}
