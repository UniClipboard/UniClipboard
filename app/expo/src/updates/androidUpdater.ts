import AsyncStorage from '@react-native-async-storage/async-storage';
import { nativeApplicationVersion, nativeBuildVersion } from 'expo-application';

import { isNativeEngineAvailable, unicEngineNative } from '@/engine/nativeModule';

export type AndroidUpdateChannel = 'stable' | 'alpha';
export type AndroidApkSource = 'r2' | 'github';

const AUTO_CHECK_KEY = '@unic:android-updates:auto-check';
const CHANNEL_KEY = '@unic:android-updates:channel';
const LAST_CHECK_DATE_KEY = '@unic:android-updates:last-check-date';

const GITHUB_DOWNLOAD_BASE = 'https://github.com/UniClipboard/UniClipboard/releases/download';

export interface AndroidUpdateSettings {
  autoCheckUpdate: boolean;
  updateChannel: AndroidUpdateChannel;
}

export interface AndroidReleaseArtifact {
  architecture?: string;
  format?: string;
  filename: string;
  downloadUrl: string;
  githubDownloadUrl: string;
  sha256?: string;
  size?: number;
}

export interface AndroidUpdateCheckResult {
  hasUpdate: boolean;
  channel: AndroidUpdateChannel;
  latestVersion: string;
  latestBuildNumber: number;
  displayVersion: string;
  versionKey: string;
  tagName: string;
  artifact: AndroidReleaseArtifact | null;
  releaseNotes?: string;
}

interface NumberedAndroidManifest {
  product?: string;
  version?: string;
  buildNumber?: string | number;
  tagName?: string;
  notes?: { en?: string; zh?: string };
  artifacts?: Array<{
    architecture?: string;
    format?: string;
    filename?: string;
    downloadUrl?: string;
    sha256?: string;
    size?: number;
  }>;
}

const SUPPORTED_ABI_PRIORITY = ['arm64-v8a', 'x86_64', 'armeabi-v7a'] as const;

export function parseSemver(version: string): [number, number, number] | null {
  const match = version.trim().match(/^v?(\d+)\.(\d+)\.(\d+)/);
  if (!match) return null;
  return [
    Number.parseInt(match[1], 10),
    Number.parseInt(match[2], 10),
    Number.parseInt(match[3], 10),
  ];
}

export function parseBuildNumber(value: string | number | null | undefined): number {
  if (typeof value === 'number' && Number.isInteger(value) && value >= 0) return value;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (/^\d+$/.test(trimmed)) return Number.parseInt(trimmed, 10);
  }
  return 0;
}

export function compareAndroidReleaseIdentity(
  candidateVersion: string,
  candidateBuild: number,
  currentVersion: string,
  currentBuild: number
): number {
  const cand = parseSemver(candidateVersion);
  const curr = parseSemver(currentVersion);
  if (cand && curr) {
    for (let i = 0; i < 3; i += 1) {
      if (cand[i] !== curr[i]) return cand[i] - curr[i];
    }
  }
  return candidateBuild - currentBuild;
}

export function getInstalledAndroidIdentity(): {
  version: string;
  buildNumber: number;
  displayVersion: string;
} {
  const version = nativeApplicationVersion ?? '0.0.0';
  const buildNumber = parseBuildNumber(nativeBuildVersion);
  const displayVersion =
    buildNumber > 0 && version !== '0.0.0' ? `${version} (${buildNumber})` : version;
  return { version, buildNumber, displayVersion };
}

export function selectAndroidArtifact(
  artifacts: AndroidReleaseArtifact[],
  supportedAbis: string[]
): AndroidReleaseArtifact | null {
  if (artifacts.length === 0) return null;
  const preferred: string[] = [];
  for (const abi of supportedAbis) {
    if ((SUPPORTED_ABI_PRIORITY as readonly string[]).includes(abi) && !preferred.includes(abi)) {
      preferred.push(abi);
    }
  }
  for (const abi of preferred) {
    const exact = artifacts.find(
      (item) => item.architecture === abi || item.filename.includes(`-${abi}.apk`)
    );
    if (exact) return exact;
  }
  const universal = artifacts.find(
    (item) => item.architecture === 'universal' || item.filename.includes('-universal.apk')
  );
  return universal ?? null;
}

function buildGitHubDownloadUrl(tagName: string, filename: string): string {
  const encodedTag = tagName
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/');
  return `${GITHUB_DOWNLOAD_BASE}/${encodedTag}/${encodeURIComponent(filename)}`;
}

function pickLocalizedNotes(
  notes: NumberedAndroidManifest['notes'] | undefined,
  language: string
): string | undefined {
  if (!notes) return undefined;
  const isChinese = language.toLowerCase().startsWith('zh');
  const primary = isChinese ? notes.zh : notes.en;
  const fallback = isChinese ? notes.en : notes.zh;
  const chosen = (primary?.trim() || fallback?.trim() || '').trim();
  return chosen || undefined;
}

export async function loadAndroidUpdateSettings(): Promise<AndroidUpdateSettings> {
  const [autoRaw, channelRaw] = await Promise.all([
    AsyncStorage.getItem(AUTO_CHECK_KEY),
    AsyncStorage.getItem(CHANNEL_KEY),
  ]);
  return {
    autoCheckUpdate: autoRaw === null ? true : autoRaw === 'true',
    updateChannel: channelRaw === 'alpha' ? 'alpha' : 'stable',
  };
}

export async function saveAutoCheckUpdate(enabled: boolean): Promise<void> {
  await AsyncStorage.setItem(AUTO_CHECK_KEY, enabled ? 'true' : 'false');
}

export async function saveUpdateChannel(channel: AndroidUpdateChannel): Promise<void> {
  await AsyncStorage.setItem(CHANNEL_KEY, channel);
}

export async function checkForAndroidUpdate(
  channel: AndroidUpdateChannel,
  language: string
): Promise<AndroidUpdateCheckResult> {
  const installed = getInstalledAndroidIdentity();
  const response = await unicEngineNative.checkAndroidUpdate(channel);

  if (!response.found || !response.manifest) {
    return {
      hasUpdate: false,
      channel,
      latestVersion: installed.version,
      latestBuildNumber: installed.buildNumber,
      displayVersion: installed.displayVersion,
      versionKey: `${installed.version}-${installed.buildNumber}`,
      tagName: '',
      artifact: null,
    };
  }

  const manifest = response.manifest as NumberedAndroidManifest;
  const latestVersion = typeof manifest.version === 'string' ? manifest.version.trim() : '';
  const latestBuildNumber = parseBuildNumber(manifest.buildNumber);
  const tagName =
    typeof manifest.tagName === 'string' && manifest.tagName.trim()
      ? manifest.tagName.trim()
      : `android/v${latestVersion}-build.${latestBuildNumber}`;

  const artifacts: AndroidReleaseArtifact[] = (manifest.artifacts ?? [])
    .filter(
      (item): item is Required<Pick<NonNullable<NumberedAndroidManifest['artifacts']>[number], 'filename' | 'downloadUrl'>> &
        NonNullable<NumberedAndroidManifest['artifacts']>[number] =>
        typeof item.filename === 'string' &&
        item.filename.endsWith('.apk') &&
        typeof item.downloadUrl === 'string' &&
        item.downloadUrl.length > 0
    )
    .map((item) => ({
      architecture: item.architecture,
      format: item.format,
      filename: item.filename,
      downloadUrl: item.downloadUrl,
      githubDownloadUrl: buildGitHubDownloadUrl(tagName, item.filename),
      sha256: item.sha256?.toLowerCase(),
      size: item.size,
    }));

  const hasUpdate =
    latestVersion.length > 0 &&
    latestBuildNumber > 0 &&
    compareAndroidReleaseIdentity(
      latestVersion,
      latestBuildNumber,
      installed.version,
      installed.buildNumber
    ) > 0;

  const displayVersion =
    latestBuildNumber > 0 ? `${latestVersion} (${latestBuildNumber})` : latestVersion;
  const versionKey = `${latestVersion}-build.${latestBuildNumber}`;
  const artifact = selectAndroidArtifact(artifacts, response.supported_abis ?? []);

  return {
    hasUpdate,
    channel,
    latestVersion,
    latestBuildNumber,
    displayVersion,
    versionKey,
    tagName,
    artifact,
    releaseNotes: hasUpdate ? pickLocalizedNotes(manifest.notes, language) : undefined,
  };
}

export async function checkForAutomaticAndroidUpdate(
  language: string
): Promise<AndroidUpdateCheckResult | null> {
  if (!isNativeEngineAvailable()) return null;
  const settings = await loadAndroidUpdateSettings();
  if (!settings.autoCheckUpdate) return null;

  const today = new Date().toISOString().slice(0, 10);
  const lastCheckDate = (await AsyncStorage.getItem(LAST_CHECK_DATE_KEY)) ?? '';
  if (lastCheckDate === today) return null;

  await AsyncStorage.setItem(LAST_CHECK_DATE_KEY, today);
  return checkForAndroidUpdate(settings.updateChannel, language);
}

export async function downloadAndInstallAndroidUpdate(
  result: AndroidUpdateCheckResult,
  source: AndroidApkSource
): Promise<{ status: 'installer_launched' | 'needs_install_permission'; cached: boolean }> {
  if (!result.artifact) {
    throw new Error('NO_SUITABLE_APK');
  }
  const url = source === 'r2' ? result.artifact.downloadUrl : result.artifact.githubDownloadUrl;
  const outcome = await unicEngineNative.downloadAndInstallAndroidUpdate(
    url,
    result.artifact.filename,
    result.artifact.sha256 ?? null,
    result.versionKey
  );
  return { status: outcome.status, cached: outcome.cached };
}

export function cancelAndroidUpdateDownload(): Promise<void> {
  return unicEngineNative.cancelAndroidUpdateDownload();
}

export function cleanOldAndroidUpdateCache(keepVersionKey?: string | null): Promise<void> {
  return unicEngineNative.cleanAndroidUpdateCache(keepVersionKey ?? null);
}
