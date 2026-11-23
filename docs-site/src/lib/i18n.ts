import { defineI18n } from 'fumadocs-core/i18n'

export const i18n = defineI18n({
  defaultLanguage: 'zh',
  // 中文先行；英文内容补齐后恢复 ['en', 'zh'] 且默认 en。
  languages: ['zh'],
  hideLocale: 'default-locale',
  parser: 'dir',
})
