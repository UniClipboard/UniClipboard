import { Laptop, Monitor, Smartphone, Tablet } from 'lucide-react'
import type { ContentTypes } from '@/api/p2p'

export function getDeviceIcon(deviceName?: string | null) {
  const name = deviceName?.toLowerCase() || ''
  if (name.includes('iphone') || name.includes('phone') || name.includes('android'))
    return Smartphone
  if (name.includes('ipad') || name.includes('tablet')) return Tablet
  if (
    name.includes('mac') ||
    name.includes('macbook') ||
    name.includes('pc') ||
    name.includes('windows')
  )
    return Laptop
  return Monitor
}

export function getIconColor(index: number) {
  const colors = [
    'text-blue-500 bg-blue-500/10 border-blue-500/20',
    'text-purple-500 bg-purple-500/10 border-purple-500/20',
    'text-green-500 bg-green-500/10 border-green-500/20',
    'text-orange-500 bg-orange-500/10 border-orange-500/20',
    'text-primary bg-primary/10 border-primary/20',
  ]
  return colors[index % colors.length]
}

/** Maps ContentTypes fields to i18n keys */
export const contentTypeEntries: {
  field: keyof ContentTypes
  i18nKey: string
  status: 'editable' | 'coming_soon'
}[] = [
  { field: 'text', i18nKey: 'syncText', status: 'editable' },
  { field: 'image', i18nKey: 'syncImage', status: 'editable' },
  { field: 'file', i18nKey: 'syncFile', status: 'editable' },
  { field: 'link', i18nKey: 'syncLink', status: 'editable' },
  { field: 'code_snippet', i18nKey: 'syncCodeSnippet', status: 'coming_soon' },
  { field: 'rich_text', i18nKey: 'syncRichText', status: 'coming_soon' },
]
