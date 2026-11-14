/**
 * i18next 资源汇总。
 *
 * 每个命名空间对应一个领域(见 locales/<lang>/<ns>.json),按领域拆分便于维护与并行迁移。
 * 新增命名空间时:在所有受支持语言目录下建同名 JSON,再在此文件登记 import 与 NS_LIST。
 */
import type { AppLanguage } from './languages';

import zhCommon from './locales/zh/common.json';
import zhOnboarding from './locales/zh/onboarding.json';
import zhHome from './locales/zh/home.json';
import zhShare from './locales/zh/share.json';
import zhSync from './locales/zh/sync.json';
import zhHistory from './locales/zh/history.json';
import zhErrors from './locales/zh/errors.json';
import zhSettings from './locales/zh/settings.json';
import zhSettingsSync from './locales/zh/settingsSync.json';
import zhSettingsBackground from './locales/zh/settingsBackground.json';
import zhSettingsStorage from './locales/zh/settingsStorage.json';
import zhSettingsAbout from './locales/zh/settingsAbout.json';
import zhSettingsPermissions from './locales/zh/settingsPermissions.json';
import zhSettingsIos from './locales/zh/settingsIos.json';

import enCommon from './locales/en/common.json';
import enOnboarding from './locales/en/onboarding.json';
import enHome from './locales/en/home.json';
import enShare from './locales/en/share.json';
import enSync from './locales/en/sync.json';
import enHistory from './locales/en/history.json';
import enErrors from './locales/en/errors.json';
import enSettings from './locales/en/settings.json';
import enSettingsSync from './locales/en/settingsSync.json';
import enSettingsBackground from './locales/en/settingsBackground.json';
import enSettingsStorage from './locales/en/settingsStorage.json';
import enSettingsAbout from './locales/en/settingsAbout.json';
import enSettingsPermissions from './locales/en/settingsPermissions.json';
import enSettingsIos from './locales/en/settingsIos.json';

import ruCommon from './locales/ru/common.json';
import ruOnboarding from './locales/ru/onboarding.json';
import ruHome from './locales/ru/home.json';
import ruShare from './locales/ru/share.json';
import ruSync from './locales/ru/sync.json';
import ruHistory from './locales/ru/history.json';
import ruErrors from './locales/ru/errors.json';
import ruSettings from './locales/ru/settings.json';
import ruSettingsSync from './locales/ru/settingsSync.json';
import ruSettingsBackground from './locales/ru/settingsBackground.json';
import ruSettingsStorage from './locales/ru/settingsStorage.json';
import ruSettingsAbout from './locales/ru/settingsAbout.json';
import ruSettingsPermissions from './locales/ru/settingsPermissions.json';
import ruSettingsIos from './locales/ru/settingsIos.json';

import ptBRCommon from './locales/pt-BR/common.json';
import ptBROnboarding from './locales/pt-BR/onboarding.json';
import ptBRHome from './locales/pt-BR/home.json';
import ptBRShare from './locales/pt-BR/share.json';
import ptBRSync from './locales/pt-BR/sync.json';
import ptBRHistory from './locales/pt-BR/history.json';
import ptBRErrors from './locales/pt-BR/errors.json';
import ptBRSettings from './locales/pt-BR/settings.json';
import ptBRSettingsSync from './locales/pt-BR/settingsSync.json';
import ptBRSettingsBackground from './locales/pt-BR/settingsBackground.json';
import ptBRSettingsStorage from './locales/pt-BR/settingsStorage.json';
import ptBRSettingsAbout from './locales/pt-BR/settingsAbout.json';
import ptBRSettingsPermissions from './locales/pt-BR/settingsPermissions.json';
import ptBRSettingsIos from './locales/pt-BR/settingsIos.json';

/** 默认命名空间:未指定 namespace 时 useTranslation()/t() 取此处 */
export const DEFAULT_NS = 'common' as const;

/** 全部命名空间列表(供 i18next init 的 ns 选项) */
export const NS_LIST = [
  'common',
  'onboarding',
  'home',
  'share',
  'sync',
  'history',
  'errors',
  'settings',
  'settingsSync',
  'settingsBackground',
  'settingsStorage',
  'settingsAbout',
  'settingsPermissions',
  'settingsIos',
] as const;

export const resources: Record<AppLanguage, Record<string, object>> = {
  'zh-CN': {
    common: zhCommon,
    onboarding: zhOnboarding,
    home: zhHome,
    share: zhShare,
    sync: zhSync,
    history: zhHistory,
    errors: zhErrors,
    settings: zhSettings,
    settingsSync: zhSettingsSync,
    settingsBackground: zhSettingsBackground,
    settingsStorage: zhSettingsStorage,
    settingsAbout: zhSettingsAbout,
    settingsPermissions: zhSettingsPermissions,
    settingsIos: zhSettingsIos,
  },
  en: {
    common: enCommon,
    onboarding: enOnboarding,
    home: enHome,
    share: enShare,
    sync: enSync,
    history: enHistory,
    errors: enErrors,
    settings: enSettings,
    settingsSync: enSettingsSync,
    settingsBackground: enSettingsBackground,
    settingsStorage: enSettingsStorage,
    settingsAbout: enSettingsAbout,
    settingsPermissions: enSettingsPermissions,
    settingsIos: enSettingsIos,
  },
  ru: {
    common: ruCommon,
    onboarding: ruOnboarding,
    home: ruHome,
    share: ruShare,
    sync: ruSync,
    history: ruHistory,
    errors: ruErrors,
    settings: ruSettings,
    settingsSync: ruSettingsSync,
    settingsBackground: ruSettingsBackground,
    settingsStorage: ruSettingsStorage,
    settingsAbout: ruSettingsAbout,
    settingsPermissions: ruSettingsPermissions,
    settingsIos: ruSettingsIos,
  },
  'pt-BR': {
    common: ptBRCommon,
    onboarding: ptBROnboarding,
    home: ptBRHome,
    share: ptBRShare,
    sync: ptBRSync,
    history: ptBRHistory,
    errors: ptBRErrors,
    settings: ptBRSettings,
    settingsSync: ptBRSettingsSync,
    settingsBackground: ptBRSettingsBackground,
    settingsStorage: ptBRSettingsStorage,
    settingsAbout: ptBRSettingsAbout,
    settingsPermissions: ptBRSettingsPermissions,
    settingsIos: ptBRSettingsIos,
  },
};
