/**
 * APK Download Service
 * 负责检测设备 ABI、下载 APK（含进度回调）、校验 SHA-256 哈希、安装 APK
 */

import { Paths, Directory, File } from 'expo-file-system';
import type { ReleaseAssetInfo } from './UpdateService';
import { parseVersion, compareVersions } from './UpdateService';

// APK 缓存目录：CLIPBOARD_TEMP_DIR/updates/v{version}/
// 清除缓存时会被一并清除
function getUpdateCacheDir(version: string): Directory {
  return new Directory(Paths.cache, 'temp_files', 'updates', `v${version}`);
}

/** 确保 APK 缓存目录链存在 */
function ensureUpdateDirExists(version: string): void {
  // 逐级创建：temp_files → updates → v{version}
  const dirs = [
    new Directory(Paths.cache, 'temp_files'),
    new Directory(Paths.cache, 'temp_files', 'updates'),
    new Directory(Paths.cache, 'temp_files', 'updates', `v${version}`),
  ];
  for (const dir of dirs) {
    if (!dir.exists) {
      try {
        dir.create();
      } catch (e) {
        if (!dir.exists) throw e; // 真实错误才抛出
      }
    }
  }
}

export function getApkCachePath(version: string, fileName: string): File {
  return new File(getUpdateCacheDir(version), fileName);
}

/**
 * 清除当前版本及更旧版本的 APK 缓存目录
 * @param currentVersion 当前 app 版本字符串（如 "1.0.11"）
 */
export function cleanOldApkCache(currentVersion: string): void {
  const updatesDir = new Directory(Paths.cache, 'temp_files', 'updates');
  if (!updatesDir.exists) return;

  const currentParsed = parseVersion(currentVersion);
  if (!currentParsed) return;

  try {
    for (const item of updatesDir.list()) {
      // 目录名格式为 v{version}
      const name = item.uri.split('/').filter(Boolean).pop() ?? '';
      if (!name.startsWith('v')) continue;
      const versionStr = name.slice(1);
      const parsed = parseVersion(versionStr);
      if (!parsed) continue;
      // 删除 <= 当前版本的目录
      if (compareVersions(parsed, currentParsed) <= 0) {
        try {
          (item as Directory).delete();
          console.log(`[ApkCache] deleted old cache: ${name}`);
        } catch (e) {
          console.warn(`[ApkCache] failed to delete ${name}:`, e);
        }
      }
    }
  } catch (e) {
    console.warn('[ApkCache] cleanOldApkCache error:', e);
  }
}

export type ApkSource = 'github' | 'gitcode';

const SUPPORTED_ABI_NAMES = ['arm64-v8a', 'armeabi-v7a', 'x86_64'] as const;
type ApkAbi = (typeof SUPPORTED_ABI_NAMES)[number] | 'universal';

/**
 * 根据设备支持的 ABI 列表，返回最优先使用的 ABI
 */
export function getPreferredAbi(supportedAbis: string[]): ApkAbi {
  for (const abi of supportedAbis) {
    if ((SUPPORTED_ABI_NAMES as readonly string[]).includes(abi)) {
      return abi as ApkAbi;
    }
  }
  return 'universal';
}

/**
 * 从 assets 列表中找到匹配指定 ABI 的 APK 资产
 * 优先精确匹配，找不到时回退到 universal
 */
export function findAssetForAbi(
  assets: ReleaseAssetInfo[],
  abi: ApkAbi
): ReleaseAssetInfo | undefined {
  const exact = assets.find((a) => a.name.includes(`-${abi}.apk`));
  if (exact) return exact;
  return assets.find((a) => a.name.includes('-universal.apk'));
}

export interface ApkDownloadProgress {
  progress: number; // 0~1
  bytesReceived: number;
  totalBytes: number;
}

export interface ApkDownloadOptions {
  asset: ReleaseAssetInfo;
  source: ApkSource;
  version: string;
  onProgress?: (info: ApkDownloadProgress) => void;
  signal?: AbortSignal;
}

/**
 * 检查 APK 是否已缓存且哈希匹配
 * @returns 缓存文件的 URI（file://...），或 null
 */
export async function checkApkCache(
  version: string,
  asset: ReleaseAssetInfo
): Promise<string | null> {
  const file = getApkCachePath(version, asset.name);
  if (!file.exists) return null;

  // 若没有期望哈希则认为缓存有效
  if (!asset.sha256) return file.uri;

  try {
    const { nativeCalculateFileHash } = await import('native-util');
    const hash = await nativeCalculateFileHash(file.uri);
    if (hash.toLowerCase() === asset.sha256.toLowerCase()) {
      return file.uri;
    }
    // 哈希不匹配，删除旧文件
    file.delete();
    return null;
  } catch {
    return null;
  }
}

/**
 * 下载 APK 并验证哈希
 * @returns 下载后文件的 URI（file://...）
 * @throws 下载失败、哈希校验失败或被取消时抛出异常
 */
export async function downloadApk(options: ApkDownloadOptions): Promise<string> {
  const { asset, source, version, onProgress, signal } = options;
  const url = source === 'github' ? asset.githubDownloadUrl : asset.gitcodeDownloadUrl;
  console.log(`[ApkDownload] start source=${source} url=${url}`);

  // 确保缓存目录存在（含父目录）
  const cacheDir = getUpdateCacheDir(version);
  console.log(`[ApkDownload] cacheDir=${cacheDir.uri} exists=${cacheDir.exists}`);
  ensureUpdateDirExists(version);
  console.log('[ApkDownload] cacheDir ready');

  const destFile = getApkCachePath(version, asset.name);
  console.log(`[ApkDownload] destFile=${destFile.uri} exists=${destFile.exists}`);
  // 删除已有的不完整文件
  if (destFile.exists) {
    destFile.delete();
    console.log('[ApkDownload] deleted stale file');
  }

  const { nativeDownloadFile } = await import('native-util');
  console.log('[ApkDownload] starting nativeDownloadFile...');
  try {
    await nativeDownloadFile(
      url,
      {},
      destFile.uri,
      signal,
      onProgress
        ? (info) =>
            onProgress({
              progress: info.progress,
              bytesReceived: info.bytesTransferred,
              totalBytes: info.totalBytes,
            })
        : undefined
    );
  } catch (e) {
    console.error('[ApkDownload] nativeDownloadFile failed:', e);
    throw e;
  }
  console.log('[ApkDownload] download completed, file exists:', destFile.exists);

  // 校验哈希
  if (asset.sha256) {
    console.log(`[ApkDownload] verifying hash, expected=${asset.sha256}`);
    const { nativeCalculateFileHash } = await import('native-util');
    const hash = await nativeCalculateFileHash(destFile.uri, signal);
    console.log(`[ApkDownload] actual hash=${hash}`);
    if (hash.toLowerCase() !== asset.sha256.toLowerCase()) {
      destFile.delete();
      throw new Error(`APK 哈希校验失败：期望 ${asset.sha256}，实际 ${hash}`);
    }
    console.log('[ApkDownload] hash verified OK');
  } else {
    console.log('[ApkDownload] no expected hash, skipping verification');
  }

  return destFile.uri;
}

/**
 * 通过系统 Intent 安装 APK
 */
export async function installApk(fileUri: string): Promise<void> {
  const { openFile } = await import('@/utils/fileActions');
  await openFile(fileUri);
}
