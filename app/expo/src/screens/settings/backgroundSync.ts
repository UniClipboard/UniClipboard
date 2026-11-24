import type { ShellGroup } from "@/shell/settings";

export const supportsBackgroundSync = false;

export function useBackgroundSyncGroups(_enabled: boolean): ShellGroup[] {
  return [];
}
