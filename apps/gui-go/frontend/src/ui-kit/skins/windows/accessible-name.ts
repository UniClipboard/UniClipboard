// Visible text names a control; `label` names only icon-only controls, whose tooltip defaults to
// it. A tooltip that repeats the name is visual (`inaccessible`) so screen readers do not announce
// it twice; a tooltip with other text is a description.
export function nameProps(iconOnly: boolean, label?: string, tooltip?: string) {
  const ariaLabel = iconOnly ? label : undefined
  const content = tooltip ?? ariaLabel
  return {
    ariaLabel,
    tooltip: content,
    relationship: content === ariaLabel ? ('inaccessible' as const) : ('description' as const),
  }
}
