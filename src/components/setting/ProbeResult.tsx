import { Check, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import type { ProbeStatus } from './relay-probe'
interface ProbeResultProps {
  status: ProbeStatus
}

export function ProbeResult({ status }: ProbeResultProps) {
  const { t } = useTranslation()
  if (status.kind === 'idle' || status.kind === 'testing') return null
  const success = status.kind === 'success'
  const message = success
    ? t('settings.sections.network.customRelays.testSuccess', { latencyMs: status.latencyMs })
    : status.message
  return (
    <p
      className={cn(
        'mt-2 flex items-center gap-1.5 text-xs',
        success ? 'text-emerald-700 dark:text-emerald-400' : 'text-destructive'
      )}
      role={success ? 'status' : 'alert'}
    >
      {success ? (
        <Check aria-hidden="true" className="size-3.5" />
      ) : (
        <TriangleAlert aria-hidden="true" className="size-3.5" />
      )}
      {message}
    </p>
  )
}
