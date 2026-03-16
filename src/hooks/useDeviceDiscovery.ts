import { useCallback, useEffect, useRef, useState } from 'react'
import {
  getP2PPeers,
  onP2PPeerConnectionChanged,
  onP2PPeerDiscoveryChanged,
  onP2PPeerNameUpdated,
} from '@/api/p2p'

/**
 * Raw peer data from backend. deviceName is null when backend has not yet
 * resolved the device name. The hook stores raw values -- the render layer
 * is responsible for displaying a localized fallback.
 */
export interface DiscoveredPeer {
  id: string
  deviceName: string | null
  device_type: string
}

/**
 * Scanning state machine:
 *   'scanning'   -- initial state, waiting for devices or timeout
 *   'hasDevices' -- at least one device is in the list
 *   'empty'      -- 10s timeout elapsed and no devices found
 */
export type ScanPhase = 'scanning' | 'hasDevices' | 'empty'

export interface UseDeviceDiscoveryOptions {
  onError?: (error: Error) => void
}

export function useDeviceDiscovery(
  active: boolean,
  options?: UseDeviceDiscoveryOptions
): { peers: DiscoveredPeer[]; scanPhase: ScanPhase; resetScan: () => void } {
  const [peers, setPeers] = useState<DiscoveredPeer[]>([])
  const [scanPhase, setScanPhase] = useState<ScanPhase>('scanning')
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Store onError in a ref so the main effect does not re-subscribe when the
  // callback identity changes between renders.
  const onErrorRef = useRef(options?.onError)

  // Sync ref in an effect (not during render, per react-hooks/refs rule)
  useEffect(() => {
    onErrorRef.current = options?.onError
  })

  /** Start (or restart) the 10-second empty-state timeout. */
  const startTimeout = useCallback(() => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current)
    }
    timeoutRef.current = setTimeout(() => {
      setScanPhase(prev => (prev === 'scanning' ? 'empty' : prev))
    }, 10_000)
  }, [])

  /** Fetch the current peer list and populate state. */
  const loadPeers = useCallback(async () => {
    try {
      const list = await getP2PPeers()
      const discovered: DiscoveredPeer[] = list.map(p => ({
        id: p.peerId,
        deviceName: p.deviceName ?? null,
        device_type: 'desktop',
      }))
      setPeers(discovered)
      if (discovered.length > 0) {
        setScanPhase('hasDevices')
      }
    } catch (err) {
      console.error('Failed to fetch peers:', err)
      const error = err instanceof Error ? err : new Error(String(err))
      onErrorRef.current?.(error)
      // Do NOT transition to 'empty' on fetch error -- timeout handles that
      setScanPhase('scanning')
    }
  }, [])

  /** Public API: reset to scanning state and re-fetch. */
  const resetScan = useCallback(() => {
    setPeers([])
    setScanPhase('scanning')
    startTimeout()
    void loadPeers()
  }, [startTimeout, loadPeers])

  useEffect(() => {
    if (!active) {
      // Deactivation reset: clear stale data so re-entry starts fresh
      setPeers([])
      setScanPhase('scanning')
      return
    }

    let cancelled = false

    // Reset state on entry so re-entry always starts clean
    setPeers([])
    setScanPhase('scanning')

    // Start the 10-second timeout
    startTimeout()

    // Initial peer load
    void loadPeers()

    // --- Event listener 1: peer discovery changes ---
    const discoveryPromise = onP2PPeerDiscoveryChanged(event => {
      if (cancelled) return
      if (event.discovered) {
        // Add or update peer (upsert by peerId)
        setPeers(prev => {
          const idx = prev.findIndex(p => p.id === event.peerId)
          const updated: DiscoveredPeer = {
            id: event.peerId,
            deviceName: event.deviceName ?? null,
            device_type: 'desktop',
          }
          if (idx >= 0) {
            const next = [...prev]
            next[idx] = updated
            return next
          }
          return [...prev, updated]
        })
        setScanPhase('hasDevices')
      } else {
        // Remove peer by peerId; transition to empty if list becomes empty
        setPeers(prev => {
          const next = prev.filter(p => p.id !== event.peerId)
          if (next.length === 0) {
            setScanPhase('empty')
          }
          return next
        })
      }
    })

    // --- Event listener 2: peer name updated ---
    const namePromise = onP2PPeerNameUpdated(event => {
      if (cancelled) return
      setPeers(prev => {
        const idx = prev.findIndex(p => p.id === event.peerId)
        if (idx < 0) return prev
        const next = [...prev]
        next[idx] = { ...next[idx], deviceName: event.deviceName }
        return next
      })
      // No scanPhase change on name update
    })

    // --- Event listener 3: peer connection changed ---
    // Per design: do NOT remove devices on disconnect -- silent update only
    const connectionPromise = onP2PPeerConnectionChanged(_event => {
      if (cancelled) return
      // No state change needed for discovery list
    })

    return () => {
      // Reset state to prevent stale data on re-entry
      setPeers([])
      setScanPhase('scanning')
      cancelled = true
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
        timeoutRef.current = null
      }
      discoveryPromise.then(fn => fn())
      namePromise.then(fn => fn())
      connectionPromise.then(fn => fn())
    }
  }, [active, startTimeout, loadPeers])

  return { peers, scanPhase, resetScan }
}
