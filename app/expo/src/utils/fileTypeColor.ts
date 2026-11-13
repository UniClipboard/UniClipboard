/**
 * 扩展名家族色：文件卡的徽章/色块颜色按扩展名家族派生，
 * 同族文件（文档/表格/压缩包/音视频…）恒定同色，未知扩展名回退文件橙。
 */

const EXT_FAMILIES: { color: string; exts: string[] }[] = [
  { color: '#E0383E', exts: ['pdf'] },
  { color: '#2B7CD3', exts: ['doc', 'docx', 'rtf', 'pages', 'odt'] },
  { color: '#217346', exts: ['xls', 'xlsx', 'csv', 'numbers', 'ods'] },
  { color: '#D24726', exts: ['ppt', 'pptx', 'key', 'odp'] },
  { color: '#8E6E53', exts: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'] },
  { color: '#AF52DE', exts: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'] },
  { color: '#FF2D55', exts: ['mp3', 'wav', 'flac', 'aac', 'm4a', 'ogg'] },
  { color: '#34C759', exts: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'heic', 'svg', 'bmp'] },
  {
    color: '#5856D6',
    exts: ['js', 'ts', 'tsx', 'jsx', 'py', 'rs', 'go', 'java', 'kt', 'swift', 'c', 'cpp', 'h'],
  },
  { color: '#5AC8FA', exts: ['json', 'xml', 'yml', 'yaml', 'toml', 'md', 'txt', 'log', 'sh'] },
];

const EXT_COLOR_MAP = new Map<string, string>();
for (const family of EXT_FAMILIES) {
  for (const ext of family.exts) {
    EXT_COLOR_MAP.set(ext, family.color);
  }
}

const DEFAULT_FILE_COLOR = '#FF9500';

/** 大写扩展名（不含点）；无扩展名或超长（非典型扩展名）返回 '' */
export function getFileExtension(fileName?: string): string {
  if (!fileName) return '';
  const idx = fileName.lastIndexOf('.');
  if (idx <= 0 || idx === fileName.length - 1) return '';
  const ext = fileName.slice(idx + 1);
  if (ext.length > 5 || /[^0-9a-zA-Z]/.test(ext)) return '';
  return ext.toUpperCase();
}

export function getExtensionColor(ext: string): string {
  return EXT_COLOR_MAP.get(ext.toLowerCase()) ?? DEFAULT_FILE_COLOR;
}

/** 去掉扩展名的文件名，供纸面上展示（扩展名已由色块承担） */
export function stripExtension(fileName: string): string {
  const ext = getFileExtension(fileName);
  return ext ? fileName.slice(0, fileName.length - ext.length - 1) : fileName;
}
