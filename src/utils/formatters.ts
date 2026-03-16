import i18n from '@/i18n'

/**
 * 格式化文件大小为人类可读格式
 * @param bytes 文件大小（字节）
 * @returns 格式化后的文件大小字符串
 */
export const formatFileSize = (bytes?: number): string => {
  if (bytes === undefined || bytes < 0 || !Number.isFinite(bytes))
    return i18n.t('common.unknownSize')
  if (bytes === 0) return i18n.t('common.zeroBytes')

  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  return `${(bytes / Math.pow(1024, i)).toFixed(2)} ${units[i]}`
}
