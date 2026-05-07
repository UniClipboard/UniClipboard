import { defineI18nUI } from 'fumadocs-ui/i18n'
import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared'
import { i18n } from './i18n'
import { appName, gitConfig } from './shared'

export const i18nUI = defineI18nUI(i18n, {
  translations: {
    en: {
      displayName: 'English',
    },
    zh: {
      displayName: '简体中文',
      search: '搜索文档',
    },
  },
})

export function baseOptions(_locale: string): BaseLayoutProps {
  return {
    nav: {
      title: appName,
    },
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
    i18n: true,
  }
}
