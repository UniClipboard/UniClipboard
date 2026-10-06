import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { expect, it } from 'vitest'
import {
  upgradeProgressEn,
  upgradeProgressJa,
  upgradeProgressPt,
  upgradeProgressRu,
  upgradeProgressZh,
} from '@/i18n/upgrade-progress'

const html = readFileSync(resolve(__dirname, '../../index.html'), 'utf8')
const copy = JSON.parse(
  /<script type="application\/json" id="uc-splash-copy">([\s\S]*?)<\/script>/.exec(html)![1]!
) as Record<string, Record<string, string>>

const expected = (bundle: typeof upgradeProgressEn) => ({
  category: bundle.startupCategory,
  title: bundle.preparing,
  description: bundle.startingDescription,
  processing: bundle.processing,
  elapsed: bundle.elapsed.replace('{{time}}', '0:00'),
})

it('keeps the static startup screen copy in sync with the React startup screen', () => {
  expect(copy).toEqual({
    'en-US': expected(upgradeProgressEn),
    'zh-CN': expected(upgradeProgressZh),
    'zh-TW': expected(upgradeProgressZh),
    'ja-JP': expected(upgradeProgressJa),
    'ru-RU': expected(upgradeProgressRu),
    'pt-BR': expected(upgradeProgressPt),
  })
})

it('has a placeholder for every copy key used by the startup screen', () => {
  for (const key of ['category', 'title', 'description', 'processing', 'elapsed']) {
    expect(html).toContain(`data-copy="${key}"`)
  }
})
