import { useRef, useState } from 'react'
import { createLogger } from '@/lib/logger'
import type { FileSyncSettings } from '@/types/setting'

const log = createLogger('file-sync-numbers')
const MB = 1024 * 1024
const fields = {
  smallFileThreshold: { min: 1, max: 1000, unit: MB, fallback: 10, message: 'smallFileThreshold' },
  maxFileSize: { min: 1, max: 10240, unit: MB, fallback: 5120, message: 'maxFileSize' },
  fileCacheQuotaPerDevice: { min: 50, max: 10240, unit: MB, fallback: 500, message: 'cacheQuota' },
  fileRetentionHours: { min: 1, max: 720, unit: 1, fallback: 24, message: 'retentionPeriod' },
} as const
type Field = keyof typeof fields
type Draft = { value: string; revision: number }
type Drafts = Partial<Record<Field, Draft>>

function validate(field: Field, value: string, values: Record<Field, string>): string | null {
  const rule = fields[field]
  const prefix = `settings.sections.sync.fileSync.${rule.message}.errors`
  if (!value.trim()) return null
  if (!/^\d+$/.test(value)) return `${prefix}.invalid`
  const number = Number(value)
  if (number < rule.min || number > rule.max) return `${prefix}.range`
  const small = Number.parseInt(values.smallFileThreshold, 10) || 0
  const max = Number.parseInt(values.maxFileSize, 10) || 0
  if ((field === 'smallFileThreshold' || field === 'maxFileSize') && small >= max) {
    return 'settings.sections.sync.fileSync.smallFileThreshold.errors.exceedsMax'
  }
  return null
}

export function useFileSyncNumbers(
  current: FileSyncSettings | undefined,
  persist: (patch: Partial<FileSyncSettings>) => Promise<void>
) {
  const [drafts, setDrafts] = useState<Drafts>({})
  const revision = useRef(0)
  const values = {} as Record<Field, string>
  for (const field of Object.keys(fields) as Field[]) {
    const rule = fields[field]
    values[field] =
      drafts[field]?.value ??
      String(Math.round((current?.[field] ?? rule.fallback * rule.unit) / rule.unit))
  }

  const change = (field: Field, value: string) => {
    const id = ++revision.current
    setDrafts(active => ({ ...active, [field]: { value, revision: id } }))
    const nextValues = { ...values, [field]: value }
    if (!value.trim() || validate(field, value, nextValues)) return
    const patch: Partial<FileSyncSettings> = { [field]: Number(value) * fields[field].unit }
    const submitted: Drafts = { [field]: { value, revision: id } }
    // A size edit can make the other draft valid. Save both together so a
    // cleared validation error never leaves an apparently saved draft behind.
    const related =
      field === 'smallFileThreshold'
        ? 'maxFileSize'
        : field === 'maxFileSize'
          ? 'smallFileThreshold'
          : null
    if (
      related &&
      drafts[related] &&
      values[related].trim() &&
      validate(related, values[related], values) &&
      !validate(related, values[related], nextValues)
    ) {
      patch[related] = Number(values[related]) * fields[related].unit
      submitted[related] = drafts[related]
    }
    const clear = () =>
      setDrafts(active => {
        const next = { ...active }
        for (const key of Object.keys(submitted) as Field[]) {
          if (active[key]?.revision === submitted[key]?.revision) delete next[key]
        }
        return next
      })
    void persist(patch).then(clear, err => {
      log.error({ err }, 'Failed to save file sync number')
      clear()
    })
  }
  const result = {} as Record<Field, { value: string; error: string | null }>
  for (const field of Object.keys(fields) as Field[]) {
    result[field] = {
      value: values[field],
      error: drafts[field] ? validate(field, values[field], values) : null,
    }
  }
  return { fields: result, change }
}
