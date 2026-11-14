/**
 * iOS shim for @expo/ui/jetpack-compose/modifiers
 * Returns modifier config objects that the shim components can interpret.
 */

type ModifierConfig = { type: string; [key: string]: any };

export function fillMaxWidth(): ModifierConfig {
  return { type: 'fillMaxWidth' };
}

export function width(value: number): ModifierConfig {
  return { type: 'width', value };
}

export function height(value: number): ModifierConfig {
  return { type: 'height', value };
}

export function paddingAll(value: number): ModifierConfig {
  return { type: 'paddingAll', value };
}

export function menuAnchor(): ModifierConfig {
  return { type: 'menuAnchor' };
}

export function clickable(onClick: () => void): ModifierConfig {
  return { type: 'clickable', onClick };
}
