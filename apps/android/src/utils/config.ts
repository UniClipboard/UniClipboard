import { AppConfig } from '@/types/storage';

export function isHistorySyncEnabled(cfg: AppConfig | null | undefined): boolean {
  if (!cfg?.enableHistorySync) {
    return false;
  }
  if (!cfg?.servers?.length || cfg.activeServerIndex < 0) {
    return false;
  }
  const serverConfig = cfg.servers[cfg.activeServerIndex];
  return serverConfig?.type === 'syncclipboard';
}
