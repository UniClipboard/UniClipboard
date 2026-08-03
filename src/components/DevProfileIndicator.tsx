import { useEffect, useState } from 'react'
import { getDeviceMeta } from '@/api/runtime'

export function DevProfileIndicator() {
  const [profile, setProfile] = useState<string | null>(null)

  useEffect(() => {
    let active = true

    getDeviceMeta()
      .then(meta => {
        if (active && meta.developmentMode) {
          setProfile(meta.runtimeProfile)
        }
      })
      .catch(() => {
        // The indicator is optional and should stay hidden if runtime metadata is unavailable.
      })

    return () => {
      active = false
    }
  }, [])

  if (!profile) return null

  const label = `Development profile: ${profile}`

  return (
    <span
      data-tauri-drag-region
      aria-label={label}
      title={label}
      className="relative z-10 inline-flex min-w-0 max-w-40 items-center gap-1 rounded border border-border/60 bg-muted/70 px-1.5 py-0.5 font-mono text-[10px] leading-none text-muted-foreground"
    >
      <span className="shrink-0 font-medium text-foreground/70">DEV</span>
      <span aria-hidden="true" className="shrink-0 text-border">
        /
      </span>
      <span className="min-w-0 truncate">{profile}</span>
    </span>
  )
}

export default DevProfileIndicator
