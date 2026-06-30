import { createLogger } from '@/lib/logger'

const log = createLogger('sponsors-api')

/**
 * Sponsor display shape returned by the public website feed
 * (`GET https://uniclipboard.app/api/v1/sponsors`). Mirrors the wall on the
 * marketing site; admin-only fields (e.g. amount) are never exposed here.
 */
export interface Sponsor {
  /** Stable id (website DB row id) — used as a React key. */
  id: string
  /** Display name. */
  name: string
  /** Optional link to their homepage / profile. */
  url?: string
  /** Optional month they started sponsoring, e.g. "2026-01". */
  since?: string
  /** Optional one-line shout-out shown under the name. */
  note?: string
  /** Optional absolute avatar URL; absent → fall back to an initials monogram. */
  avatar?: string
  /** "gold" sponsors are highlighted and sorted to the front. */
  tier?: 'gold' | 'regular'
}

interface SponsorsResponse {
  items: Sponsor[]
  count: number
}

/**
 * Public sponsors endpoint. Overridable via `VITE_SPONSORS_API_URL` for local
 * testing against a dev website instance.
 *
 * NOTE: uses the canonical `www.` host. The apex `uniclipboard.app` 308-redirects
 * to `www`, and a cross-origin redirect aborts a CORS fetch unless every hop
 * carries `Access-Control-Allow-Origin` (the edge redirect does not), so hit the
 * final host directly.
 */
const SPONSORS_API_URL =
  import.meta.env.VITE_SPONSORS_API_URL ?? 'https://www.uniclipboard.app/api/v1/sponsors'

/** Give up rather than hang the About panel on a slow/offline network. */
const FETCH_TIMEOUT_MS = 8000

/**
 * Fetch the public sponsor list directly from the website. Unlike the rest of
 * the UI this bypasses the daemon and hits an external origin: the data is
 * public, read-only, and the website serves permissive CORS for it.
 *
 * The caller's `signal` (component unmount) is honoured alongside an internal
 * timeout. Network/parse failures propagate; callers degrade gracefully.
 */
export async function fetchSponsors(signal?: AbortSignal): Promise<Sponsor[]> {
  const timeout = AbortSignal.timeout(FETCH_TIMEOUT_MS)
  const combined = signal ? AbortSignal.any([signal, timeout]) : timeout

  const res = await fetch(SPONSORS_API_URL, {
    method: 'GET',
    headers: { Accept: 'application/json' },
    signal: combined,
  })
  if (!res.ok) {
    throw new Error(`sponsors fetch failed: ${res.status}`)
  }
  const data = (await res.json()) as SponsorsResponse
  log.debug({ count: data.count }, 'fetched sponsors')
  return data.items ?? []
}
