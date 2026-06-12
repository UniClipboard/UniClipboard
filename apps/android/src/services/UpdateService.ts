/**
 * Update Service
 * 检查 GitHub 最新 Release 版本更新
 */

const GITHUB_RELEASES_API = 'https://api.github.com/repos/UniClipboard/uc-android/releases';
const RELEASES_PAGE_URL = 'https://github.com/UniClipboard/uc-android/releases';
// NOTE: GitCode 的 release 资源下载 URL 模式按 GitHub 风格猜测,
// 首次在 GitCode 发布 release 后,必须人工点开下载链接验证路径是否对得上,
// 若 GitCode 实际使用其他模式(如 GitLab 风格 /-/releases/<tag>/downloads/<file>),需相应调整下方两行模板
const GITCODE_RELEASES_PAGE_URL = 'https://gitcode.com/UniClipboard/uc-android/releases';
const GITCODE_DOWNLOAD_BASE = 'https://gitcode.com/UniClipboard/uc-android/releases/download';

export interface ParsedVersion {
  major: number;
  minor: number;
  patch: number;
  build?: number;
  beta?: number;
}

export interface ReleaseAssetInfo {
  /** APK 文件名，如 UniClip-1.0.11-arm64-v8a.apk */
  name: string;
  /** GitHub 直接下载 URL */
  githubDownloadUrl: string;
  /** GitCode 直接下载 URL */
  gitcodeDownloadUrl: string;
  /** SHA-256 哈希值（十六进制小写），来自 GitHub API digest 字段，可能为 undefined */
  sha256?: string;
}

export function versionToStr(v: ParsedVersion): string {
  let s = `${v.major}.${v.minor}.${v.patch}`;
  if (v.build !== undefined) s += `.${v.build}`;
  if (v.beta !== undefined) s += `-beta${v.beta}`;
  return s;
}

/**
 * 解析版本字符串，支持格式：
 *   v1.2.3, 1.2.3, v1.2.3.4, v1.2.3-beta1, 1.2.3.4-beta2
 */
export function parseVersion(versionStr: string): ParsedVersion | null {
  const match = versionStr.trim().match(/^v?(\d+)\.(\d+)\.(\d+)(?:\.(\d+))?(?:-beta(\d+))?$/i);
  if (!match) return null;
  return {
    major: parseInt(match[1], 10),
    minor: parseInt(match[2], 10),
    patch: parseInt(match[3], 10),
    build: match[4] !== undefined ? parseInt(match[4], 10) : undefined,
    beta: match[5] !== undefined ? parseInt(match[5], 10) : undefined,
  };
}

/**
 * 比较两个版本，返回:
 *   正数 => a > b，负数 => a < b，0 => 相等
 * 规则与 AppVersion.cs 一致：正式版 > beta 版
 */
export function compareVersions(a: ParsedVersion, b: ParsedVersion): number {
  const nums: (keyof ParsedVersion)[] = ['major', 'minor', 'patch'];
  for (const key of nums) {
    const av = (a[key] as number | undefined) ?? 0;
    const bv = (b[key] as number | undefined) ?? 0;
    if (av !== bv) return av - bv;
  }

  // build 段（第四位）
  const aBuild = a.build ?? -1;
  const bBuild = b.build ?? -1;
  if (aBuild !== bBuild) {
    // 有 build 段的字段数更多，视为更大
    if (a.build !== undefined && b.build === undefined) return 1;
    if (a.build === undefined && b.build !== undefined) return -1;
    return aBuild - bBuild;
  }

  // beta：正式版 (beta === undefined) > beta 版
  if (a.beta === undefined && b.beta === undefined) return 0;
  if (a.beta === undefined) return 1; // a 是正式版，更大
  if (b.beta === undefined) return -1; // b 是正式版，更大
  return a.beta - b.beta;
}

export interface UpdateCheckResult {
  hasUpdate: boolean;
  latestVersion: string;
  tagName: string;
  releaseUrl: string;
  gitcodeReleaseUrl: string;
  /** APK 资源列表（含各 ABI 的下载 URL 和哈希值） */
  assets: ReleaseAssetInfo[];
  /** GitHub Release 更新说明 */
  releaseNotes?: string;
}

/**
 * 从 GitHub API 获取最新版本并与当前版本比较
 * @param currentVersionStr 当前版本字符串
 * @param includeBeta 是否包含 beta 版本，默认 false
 */
export async function checkForUpdate(
  currentVersionStr: string,
  includeBeta = false
): Promise<UpdateCheckResult> {
  const response = await fetch(GITHUB_RELEASES_API, {
    headers: { Accept: 'application/vnd.github+json' },
  });

  if (!response.ok) {
    throw new Error(`GitHub API 请求失败: ${response.status}`);
  }

  const releases: Array<{
    tag_name: string;
    prerelease: boolean;
    draft: boolean;
    html_url: string;
    body?: string;
    assets: Array<{
      name: string;
      browser_download_url: string;
      digest?: string; // 格式: "sha256:HEX_VALUE"
    }>;
  }> = await response.json();

  // 过滤草稿；若不包含 beta 则进一步过滤 prerelease
  const candidates = releases.filter((r) => {
    if (r.draft) return false;
    if (!includeBeta && r.prerelease) return false;
    return true;
  });

  const latest = candidates[0];
  if (!latest) {
    return {
      hasUpdate: false,
      latestVersion: currentVersionStr,
      tagName: '',
      releaseUrl: RELEASES_PAGE_URL,
      gitcodeReleaseUrl: GITCODE_RELEASES_PAGE_URL,
      assets: [],
      releaseNotes: undefined,
    };
  }

  const latestParsed = parseVersion(latest.tag_name);
  const currentParsed = parseVersion(currentVersionStr);

  const apkAssets: ReleaseAssetInfo[] = latest.assets
    .filter((a) => a.name.endsWith('.apk'))
    .map((a) => ({
      name: a.name,
      githubDownloadUrl: a.browser_download_url,
      gitcodeDownloadUrl: `${GITCODE_DOWNLOAD_BASE}/${latest.tag_name}/${a.name}`,
      sha256: a.digest?.startsWith('sha256:') ? a.digest.slice(7).toLowerCase() : undefined,
    }));

  if (!currentParsed || !latestParsed) {
    return {
      hasUpdate: false,
      latestVersion: latest.tag_name,
      tagName: latest.tag_name,
      releaseUrl: latest.html_url,
      gitcodeReleaseUrl: `https://gitcode.com/UniClipboard/uc-android/releases/tag/${latest.tag_name}`,
      assets: apkAssets,
      releaseNotes: latest.body,
    };
  }

  const hasUpdate = compareVersions(latestParsed, currentParsed) > 0;
  return {
    hasUpdate,
    latestVersion: versionToStr(latestParsed),
    tagName: latest.tag_name,
    releaseUrl: latest.html_url,
    gitcodeReleaseUrl: `https://gitcode.com/UniClipboard/uc-android/releases/tag/${latest.tag_name}`,
    assets: apkAssets,
    releaseNotes: latest.body,
  };
}
