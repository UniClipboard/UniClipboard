import { expect, it } from 'vitest'
import i18n, { SUPPORTED_LANGUAGES } from '@/i18n'
import { upgradeProgressZh } from '@/i18n/upgrade-progress'

function leaves(value: Record<string, unknown>, prefix = ''): string[] {
  return Object.entries(value).flatMap(([key, entry]) => {
    const path = prefix ? `${prefix}.${key}` : key
    return typeof entry === 'string' ? [path] : leaves(entry as Record<string, unknown>, path)
  })
}
it.each(SUPPORTED_LANGUAGES)('%s provides every startup string without fallback', language => {
  for (const key of leaves(upgradeProgressZh)) {
    const translated = i18n.getResource(language, 'translation', `upgradeProgress.${key}`)
    expect(translated, `${language}:${key}`).toEqual(expect.any(String))
    const source = i18n.getResource('zh-CN', 'translation', `upgradeProgress.${key}`) as string
    expect((translated as string).match(/{{\w+}}/g)?.sort() ?? []).toEqual(
      source.match(/{{\w+}}/g)?.sort() ?? []
    )
  }
})
