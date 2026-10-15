import { memo } from 'react'
import { Switch } from '@/components/ui/switch'
import { useLocalSyncToggle } from '@/hooks/useLocalSyncToggle'
import { cn } from '@/lib/utils'

function LocalSyncToggle({ kind }: { kind: 'sync' | 'file' }) {
  const { available, checked, saving, title, description, save } = useLocalSyncToggle(kind)

  return (
    <div
      className={cn(
        'flex min-h-16 min-w-0 items-center gap-5 px-5 py-4 @md:px-6',
        !available && 'opacity-55'
      )}
    >
      <div className="min-w-0 flex-1">
        <p className="text-ui-body font-medium text-foreground">{title}</p>
        <p className="mt-1 text-ui-caption text-muted-foreground">{description}</p>
      </div>
      <Switch
        aria-label={title}
        aria-busy={saving}
        className={cn(
          'shrink-0',
          available && saving && 'disabled:cursor-pointer disabled:opacity-100'
        )}
        checked={checked}
        disabled={!available || saving}
        onCheckedChange={value => void save(value)}
      />
    </div>
  )
}

export default memo(LocalSyncToggle)
