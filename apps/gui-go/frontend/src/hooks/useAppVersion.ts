import { useEffect, useState } from 'react'
import { getVersion } from '@/host/app'

/** The running app's version, or `null` until it is known or where the host cannot tell. */
export function useAppVersion(): string | null {
  const [version, setVersion] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    getVersion()
      .then(value => {
        if (!cancelled) setVersion(value)
      })
      .catch(() => {
        // Outside the desktop host (plain browser dev) the API is unavailable; callers hide it.
      })
    return () => {
      cancelled = true
    }
  }, [])
  return version
}
