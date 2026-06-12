import { logger, consoleTransport } from 'react-native-logs';
import { Paths, Directory, File } from 'expo-file-system';
import { StorageAccessFramework } from 'expo-file-system/legacy';
import { Platform } from 'react-native';
import { nativeZipFiles } from 'native-util';
import * as Application from 'expo-application';

const LOG_DIR = new Directory(Paths.document, 'logs');
const MAX_LOG_DAYS = 3;

function formatLocalDate(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function formatLocalDateTime(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  const seconds = String(date.getSeconds()).padStart(2, '0');
  return `${year}-${month}-${day}_${hours}-${minutes}-${seconds}`;
}

function formatLocalTimestamp(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  const seconds = String(date.getSeconds()).padStart(2, '0');
  return `${year}-${month}-${day} ${hours}:${minutes}:${seconds}`;
}

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface LogConfig {
  level: LogLevel;
  enableConsole: boolean;
}

interface CustomTransportOptions {
  _custom?: string;
}

let isInitialized = false;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let logInstance: any = null;

const customFileTransport = (props: {
  msg: string;
  rawMsg: unknown;
  level: { severity: number; text: string };
  extension?: string | null;
  options?: CustomTransportOptions;
}): void => {
  try {
    if (!LOG_DIR.exists) {
      LOG_DIR.create();
    }

    const today = new Date();
    const dateStr = formatLocalDate(today);
    const fileName = `app_${dateStr}.txt`;
    const logFile = new File(LOG_DIR, fileName);

    const timestamp = formatLocalTimestamp(today);
    const level = props.level.text.toUpperCase();
    const extension = props.extension ? ` [${props.extension}]` : '';
    const message = props.msg;

    const logLine = `${timestamp} ${level}${extension}: ${message}\n`;

    if (logFile.exists) {
      const existingContent = logFile.textSync() || '';
      logFile.write(existingContent + logLine);
    } else {
      logFile.write(logLine);
    }
  } catch (error) {
    console.error('Failed to write log file:', error);
  }
};

export function initLogger(config?: Partial<LogConfig>): void {
  if (isInitialized) {
    return;
  }

  const logConfig = {
    level: config?.level ?? 'debug',
    enableConsole: config?.enableConsole ?? true,
  };

  const transports = logConfig.enableConsole
    ? [consoleTransport, customFileTransport]
    : [customFileTransport];

  logInstance = logger.createLogger({
    levels: {
      debug: 0,
      info: 1,
      warn: 2,
      error: 3,
    },
    severity: logConfig.level,
    transport: transports,
    async: true,
    dateFormat: 'iso',
    printLevel: true,
    printDate: true,
  });

  logInstance.patchConsole();
  isInitialized = true;

  cleanOldLogs();
  logSystemInfo();
}

function logSystemInfo(): void {
  if (Platform.OS !== 'android') return;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const c = (Platform as any).constants;
  const appVersion = Application.nativeApplicationVersion ?? 'unknown';
  const buildVersion = Application.nativeBuildVersion ?? 'unknown';
  const androidRelease: string = c?.Release ?? 'unknown';
  const apiLevel: number = Platform.Version as number;
  const model: string = c?.Model ?? 'unknown';
  const manufacturer: string = c?.manufacturer ?? 'unknown';
  logInstance.info(
    `App started | version: ${appVersion} (build ${buildVersion}) | ` +
      `Android ${androidRelease} (API ${apiLevel}) | ${manufacturer} ${model}`
  );
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function getLogger(): any {
  if (!logInstance) {
    initLogger();
  }
  return logInstance;
}

export function setLogLevel(level: LogLevel): void {
  if (logInstance) {
    logInstance.setSeverity(level);
  }
}

export function getLogDirectory(): Directory {
  return LOG_DIR;
}

export function getLogFilePaths(): string[] {
  if (!LOG_DIR.exists) {
    return [];
  }

  const files = LOG_DIR.list();
  return files
    .filter((entry): entry is File => entry instanceof File)
    .filter((file) => file.name.endsWith('.txt'))
    .map((file) => file.uri);
}

export function calculateLogSize(): number {
  if (!LOG_DIR.exists) {
    return 0;
  }

  let totalSize = 0;
  const files = LOG_DIR.list();

  for (const entry of files) {
    if (entry instanceof File) {
      try {
        const info = entry.info();
        totalSize += info.size || 0;
      } catch {
        // ignore
      }
    }
  }

  return totalSize;
}

export function clearLogs(): void {
  if (LOG_DIR.exists) {
    const files = LOG_DIR.list();
    for (const entry of files) {
      try {
        if (entry instanceof File) {
          entry.delete();
        }
      } catch {
        // ignore
      }
    }
  }
}

export function cleanOldLogs(): void {
  if (!LOG_DIR.exists) {
    return;
  }

  const today = new Date();
  const cutoffDate = new Date(today);
  cutoffDate.setDate(cutoffDate.getDate() - MAX_LOG_DAYS);

  const files = LOG_DIR.list();
  for (const entry of files) {
    if (entry instanceof File && entry.name.endsWith('.txt')) {
      const match = entry.name.match(/app_(\d{4}-\d{2}-\d{2})\.txt/);
      if (match) {
        const fileDate = new Date(match[1]);
        if (fileDate < cutoffDate) {
          try {
            entry.delete();
          } catch {
            // ignore
          }
        }
      }
    }
  }
}

export const log = {
  debug: (...args: unknown[]) => getLogger().debug(args.length === 1 ? args[0] : args),
  info: (...args: unknown[]) => getLogger().info(args.length === 1 ? args[0] : args),
  warn: (...args: unknown[]) => getLogger().warn(args.length === 1 ? args[0] : args),
  error: (...args: unknown[]) => getLogger().error(args.length === 1 ? args[0] : args),
};

export function getLogFileUris(): string[] {
  if (!LOG_DIR.exists) {
    return [];
  }

  return LOG_DIR.list()
    .filter((entry): entry is File => entry instanceof File && entry.name.endsWith('.txt'))
    .map((file) => file.uri);
}

export async function saveLogsToFile(signal?: AbortSignal): Promise<void> {
  if (Platform.OS !== 'android') {
    throw new Error('saveLogsToFile is only supported on Android');
  }

  const fileUris = getLogFileUris();
  if (fileUris.length === 0) {
    throw new Error('没有可导出的日志文件');
  }

  const timestamp = formatLocalDateTime(new Date());
  const zipFileName = `logs_${timestamp}`;

  const permissions = await StorageAccessFramework.requestDirectoryPermissionsAsync();
  if (!permissions.granted) {
    throw new Error('未授予存储权限');
  }

  const destUri = await StorageAccessFramework.createFileAsync(
    permissions.directoryUri,
    zipFileName,
    'application/zip'
  );

  await nativeZipFiles(fileUris, destUri, signal);
}
