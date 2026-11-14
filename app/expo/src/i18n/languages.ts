/**
 * i18n 支持的语言清单与设备语言解析。
 *
 * - AppLanguage: 应用真正加载资源的语言代码(与 locales/<code> 目录一一对应)。
 * - LanguagePreference: 用户在设置里选的偏好,'system' 表示跟随系统。
 *
 * 设备语言通过 expo-localization 读取,并映射到受支持的 AppLanguage;
 * 无法匹配时回退到 FALLBACK_LANGUAGE。
 */
import { getLocales } from 'expo-localization';

/** 应用实际加载翻译资源使用的语言代码 */
export type AppLanguage = 'zh-CN' | 'en' | 'ru' | 'pt-BR';

/** 用户偏好:'system' 跟随系统,其余为显式语言 */
export type LanguagePreference = 'system' | AppLanguage;

/** 受支持的语言(资源目录以此为准) */
export const SUPPORTED_LANGUAGES: readonly AppLanguage[] = ['zh-CN', 'en', 'ru', 'pt-BR'] as const;

/** 缺失翻译时的回退语言 */
export const FALLBACK_LANGUAGE: AppLanguage = 'en';

/**
 * 各语言用自身书写系统展示的名称(不翻译,便于用户在任意界面语言下辨认)。
 */
export const LANGUAGE_NATIVE_NAMES: Record<AppLanguage, string> = {
  'zh-CN': '简体中文',
  en: 'English',
  ru: 'Русский',
  'pt-BR': 'Português (Brasil)',
};

/**
 * 读取系统语言并映射到受支持的 AppLanguage。
 * 按用户偏好顺序遍历 getLocales(),命中第一个受支持语言即返回。
 */
export function resolveDeviceLanguage(): AppLanguage {
  try {
    for (const locale of getLocales()) {
      const code = locale.languageCode?.toLowerCase();
      if (code === 'zh') return 'zh-CN';
      if (code === 'en') return 'en';
      if (code === 'ru') return 'ru';
      if (code === 'pt') return 'pt-BR';
    }
  } catch {
    // getLocales 理论上不会抛,兜底以防 web/异常环境
  }
  return FALLBACK_LANGUAGE;
}

/**
 * 把用户偏好解析为实际加载的语言:
 * 显式语言直接返回;'system' 或未知值回退到系统语言。
 */
export function resolvePreference(
  pref: LanguagePreference | string | null | undefined
): AppLanguage {
  if (pref === 'zh-CN' || pref === 'en' || pref === 'ru' || pref === 'pt-BR') return pref;
  return resolveDeviceLanguage();
}
