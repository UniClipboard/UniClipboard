import { useEffect, useRef, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { useClipboardEventStream } from './useClipboardEventStream'
import { useEncryptionSessionState } from './useEncryptionSessionState'
import { Filter, OrderBy } from '@/api/clipboardItems'
import { toast } from '@/components/ui/toast'
import { useAppDispatch } from '@/store/hooks'
import {
  fetchClipboardItems,
  setNotReady,
  prependItem,
  removeItem,
} from '@/store/slices/clipboardSlice'
const PAGE_SIZE = 20

interface UseClipboardEventsReturn {
  hasMore: boolean
  handleLoadMore: () => void
  encryptionReady: boolean
}

export function useClipboardEvents(currentFilter: Filter): UseClipboardEventsReturn {
  const dispatch = useAppDispatch()
  const { t } = useTranslation()

  // Refs (no global module state)
  const pendingInitialLoadRef = useRef(false)
  const loadInFlightRef = useRef(false)
  const offsetRef = useRef(0)
  const hasMoreRef = useRef(true)
  const currentFilterRef = useRef<Filter>(currentFilter)
  const [hasMore, setHasMore] = useState(true)
  const { encryptionReady } = useEncryptionSessionState()

  // Load clipboard data
  const loadData = useCallback(
    async ({
      specificFilter,
      reset = false,
    }: { specificFilter?: Filter; reset?: boolean } = {}) => {
      if (loadInFlightRef.current) return
      if (!reset && !hasMoreRef.current) return

      if (reset) {
        offsetRef.current = 0
        hasMoreRef.current = true
        setHasMore(true)
      }

      const filterToUse = specificFilter || currentFilterRef.current
      console.log(t('dashboard.logs.loadingClipboard'), filterToUse)

      loadInFlightRef.current = true
      try {
        const result = await dispatch(
          fetchClipboardItems({
            orderBy: OrderBy.ActiveTimeDesc,
            filter: filterToUse,
            limit: PAGE_SIZE,
            offset: offsetRef.current,
          })
        ).unwrap()

        if (result.status === 'not_ready') {
          hasMoreRef.current = false
          setHasMore(false)
          return
        }

        const fetchedCount = result.items.length
        offsetRef.current += fetchedCount
        const nextHasMore = fetchedCount === PAGE_SIZE
        hasMoreRef.current = nextHasMore
        setHasMore(nextHasMore)
      } catch (error) {
        console.error('Failed to load clipboard data:', error)
        toast.error(t('dashboard.errors.loadFailed'), {
          description: error instanceof Error ? error.message : t('dashboard.errors.unknown'),
        })
      } finally {
        loadInFlightRef.current = false
      }
    },
    [dispatch, t]
  )

  // Filter change effect
  useEffect(() => {
    console.log(t('dashboard.logs.filterChanged'), currentFilter)
    currentFilterRef.current = currentFilter
    if (!encryptionReady) {
      pendingInitialLoadRef.current = true
      console.log('[useClipboardEvents] Encryption not ready, deferring clipboard load')
      return
    }
    pendingInitialLoadRef.current = false
    loadData({ specificFilter: currentFilter, reset: true })
  }, [currentFilter, encryptionReady, loadData, t])

  useClipboardEventStream({
    enabled: encryptionReady,
    onLocalItem: item => {
      dispatch(prependItem(item))
      offsetRef.current += 1
    },
    onRemoteInvalidate: () => {
      void loadData({ specificFilter: currentFilterRef.current, reset: true })
    },
    onDeleted: id => {
      dispatch(removeItem(id))
      offsetRef.current = Math.max(0, offsetRef.current - 1)
    },
  })

  useEffect(() => {
    if (encryptionReady) {
      dispatch(setNotReady(false))
      if (pendingInitialLoadRef.current) {
        pendingInitialLoadRef.current = false
        void loadData({ specificFilter: currentFilterRef.current, reset: true })
      }
    } else {
      dispatch(setNotReady(true))
    }
  }, [dispatch, encryptionReady, loadData])

  const handleLoadMore = useCallback(() => {
    if (!encryptionReady) return
    void loadData()
  }, [encryptionReady, loadData])

  return { hasMore, handleLoadMore, encryptionReady }
}
