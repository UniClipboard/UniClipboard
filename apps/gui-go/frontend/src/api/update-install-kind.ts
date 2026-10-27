import type { InstallKind } from '@/api/updater'

export function isPackageManagedInstall(kind: InstallKind | null): boolean {
  return kind === 'deb' || kind === 'rpm' || kind === 'pacman'
}

export function requiresExternalUpdate(kind: InstallKind | null): boolean {
  return isPackageManagedInstall(kind) || kind === 'windowsportable'
}
