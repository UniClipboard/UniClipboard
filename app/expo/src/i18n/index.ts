/**
 * i18next 初始化(应用全局单例)。
 *
 * - 导入本文件即完成初始化(副作用);在 index.ts 入口顶部 import,保证所有
 *   AppRegistry 入口(main / quickAction / serviceRestart)在渲染任何组件前就绪。
 * - 初始语言取系统语言;用户显式偏好由 App 在配置加载后经 applyLanguagePreference 应用。
 * - resources 全部静态打包,故 useSuspense 关闭,组件无需等待异步加载。
 * - 组件内用 useTranslation();非组件(store/util/Alert)用 `import i18n from '@/i18n'; i18n.t(...)`。
 */
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import { FALLBACK_LANGUAGE, SUPPORTED_LANGUAGES, resolveDeviceLanguage } from './languages';
import { DEFAULT_NS, NS_LIST, resources } from './resources';

if (!i18n.isInitialized) {
  void i18n.use(initReactI18next).init({
    resources,
    lng: resolveDeviceLanguage(),
    fallbackLng: FALLBACK_LANGUAGE,
    supportedLngs: SUPPORTED_LANGUAGES as unknown as string[],
    ns: NS_LIST as unknown as string[],
    defaultNS: DEFAULT_NS,
    // 缺失键回退到 fallbackLng 的同名键
    returnNull: false,
    returnEmptyString: false,
    interpolation: {
      // RN 无 XSS 风险,关闭转义以保留原文
      escapeValue: false,
    },
    react: {
      useSuspense: false,
    },
  });
}

export default i18n;
export { i18n };

const shellResources = {
  'zh-CN': { title: '界面演示', banner: '界面演示 · 未连接 Engine', unavailable: '此操作尚未接入。不会发送数据，也不会修改系统剪贴板。', empty: '没有匹配的演示记录', localOnly: '仅修改本次运行的演示状态。', reset: '恢复演示记录', source: '界面来源', onboarding: '查看引导页', integration: '系统集成', disconnected: '未接入', addConnection: '添加连接', demoDevices: '演示设备' },
  en: { title: 'UI demo', banner: 'UI demo · Engine disconnected', unavailable: 'This action is not integrated. No data is sent and the system clipboard is not changed.', empty: 'No matching demo records', localOnly: 'Changes affect demo state for this run only.', reset: 'Reset demo history', source: 'UI source', onboarding: 'Preview onboarding', integration: 'System integration', disconnected: 'Not integrated', addConnection: 'Add connection', demoDevices: 'Demo devices' },
  ru: { title: 'Демонстрация', banner: 'Демонстрация · Engine не подключён', unavailable: 'Функция не подключена. Данные не отправляются, буфер обмена не изменяется.', empty: 'Нет подходящих демонстрационных записей', localOnly: 'Изменения действуют только в этом сеансе.', reset: 'Восстановить демонстрационные записи', source: 'Источник интерфейса', onboarding: 'Показать введение', integration: 'Системная интеграция', disconnected: 'Не подключено', addConnection: 'Добавить подключение', demoDevices: 'Демонстрационные устройства' },
  'pt-BR': { title: 'Demonstração', banner: 'Demonstração · Engine desconectado', unavailable: 'Esta ação ainda não está integrada. Nenhum dado é enviado e a área de transferência não é alterada.', empty: 'Nenhum registro de demonstração encontrado', localOnly: 'As alterações afetam apenas esta sessão.', reset: 'Restaurar registros de demonstração', source: 'Origem da interface', onboarding: 'Ver introdução', integration: 'Integração do sistema', disconnected: 'Não integrado', addConnection: 'Adicionar conexão', demoDevices: 'Dispositivos de demonstração' },
};
for (const [language, values] of Object.entries(shellResources)) i18n.addResourceBundle(language, 'shell', values);
