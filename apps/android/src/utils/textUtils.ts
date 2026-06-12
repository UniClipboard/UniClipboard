/**
 * Text Utilities
 * 文本相关的工具函数
 */

/**
 * Check if text is invalid (undefined or null)
 * Empty string is considered valid
 */
export const isTextInvalid = (text: unknown): text is undefined | null => {
  return text === undefined || text === null;
};
