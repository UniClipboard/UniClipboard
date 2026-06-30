export function getFileExtLabel(name: string): string {
  const dot = name.lastIndexOf('.')
  // No extension (e.g. `README`, `LICENSE`) or a trailing dot: fall back to the
  // generic badge instead of rendering the whole basename.
  if (dot <= 0 || dot === name.length - 1) return 'FILE'
  return name.slice(dot + 1).toUpperCase()
}
