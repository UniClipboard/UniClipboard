import { listen } from '@tauri-apps/api/event'
import React, { useState, useEffect, useRef, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, OrderBy } from '@/api/clipboardItems'
import { getEncryptionSessionStatus } from '@/api/security'
import ClipboardContent from '@/components/clipboard/ClipboardContent'
import Header from '@/components/layout/Header'
import { toast } from '@/components/ui/toast'
import { useSearch } from '@/contexts/search-context'
import { useLifecycleStatus } from '@/hooks/useLifecycleStatus'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { useAppDispatch } from '@/store/hooks'
import { fetchClipboardItems, setNotReady } from '@/store/slices/clipboardSlice'
import { ClipboardEvent } from '@/types/events'

// Debounce delay in milliseconds
const DEBOUNCE_DELAY = 500
const PAGE_SIZE = 20

// Global listener state management
interface ListenerState {
  isActive: boolean
  unlisten?: () => void
  lastEventTimestamp?: number
}

const globalListenerState: ListenerState = {
  isActive: false,
}

const DashboardPage: React.FC = () => {
  const { t } = useTranslation()
  const { searchValue } = useSearch()
  const [currentFilter, setCurrentFilter] = useState<Filter>(Filter.All)
  const dispatch = useAppDispatch()
  const { status: lifecycleStatus, retry: retryLifecycle, retrying } = useLifecycleStatus()

  // 设置当前页面作用域为 clipboard
  useShortcutScope('clipboard')

  // Use ref to store the latest filter value
  const currentFilterRef = useRef<Filter>(currentFilter)
  // Debounce ref
  const debouncedLoadRef = useRef<number | null>(null)
  const encryptionReadyRef = useRef<boolean | null>(null)
  const pendingInitialLoadRef = useRef(false)
  const loadInFlightRef = useRef(false)
  const offsetRef = useRef(0)
  const hasMoreRef = useRef(true)
  const [hasMore, setHasMore] = useState(true)

  const handleFilterChange = (filterId: Filter) => {
    setCurrentFilter(filterId)
  }

  // Load clipboard records and statistics
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
        console.error('加载剪贴板数据失败:', error)
        toast.error(t('dashboard.errors.loadFailed'), {
          description: error instanceof Error ? error.message : t('dashboard.errors.unknown'),
        })
      } finally {
        loadInFlightRef.current = false
      }
    },
    [dispatch, t]
  )

  // Debounced data loading
  const debouncedLoadData = useCallback(
    (specificFilter?: Filter) => {
      if (debouncedLoadRef.current) {
        clearTimeout(debouncedLoadRef.current)
      }

      debouncedLoadRef.current = setTimeout(() => {
        loadData({ specificFilter, reset: true })
        debouncedLoadRef.current = null
      }, DEBOUNCE_DELAY)
    },
    [loadData]
  )

  // Update ref to track the latest filter
  useEffect(() => {
    console.log(t('dashboard.logs.filterChanged'), currentFilter)
    currentFilterRef.current = currentFilter
    if (encryptionReadyRef.current !== true) {
      pendingInitialLoadRef.current = true
      console.log('[Dashboard] Encryption not ready, deferring clipboard load')
      return
    }
    pendingInitialLoadRef.current = false
    loadData({ specificFilter: currentFilter, reset: true })
  }, [currentFilter, loadData, t])

  // Setup clipboard content listener
  useEffect(() => {
    // Function to setup listener
    const setupListener = async () => {
      // Only setup if there's no active listener yet
      if (!globalListenerState.isActive) {
        console.log(t('dashboard.logs.settingGlobalListener'))
        globalListenerState.isActive = true

        try {
          console.log(t('dashboard.logs.listeningToClipboardEvents'))
          // Clear previously existing listener
          if (globalListenerState.unlisten) {
            console.log(t('dashboard.logs.clearingPreviousListener'))
            globalListenerState.unlisten()
            globalListenerState.unlisten = undefined
          }

          // Listen to new clipboard://event format
          const unlisten = await listen<ClipboardEvent>('clipboard://event', event => {
            console.log(t('dashboard.logs.newClipboardEvent'), event)

            // Check event type
            if (event.payload.type === 'NewContent' && event.payload.entry_id) {
              // Check event timestamp to avoid processing duplicate events within short time
              const currentTime = Date.now()
              if (
                globalListenerState.lastEventTimestamp &&
                currentTime - globalListenerState.lastEventTimestamp < DEBOUNCE_DELAY
              ) {
                console.log(t('dashboard.logs.ignoringDuplicateEvent'))
                return
              }

              // Update last event timestamp
              globalListenerState.lastEventTimestamp = currentTime

              // Use debounced function to load data
              debouncedLoadData(currentFilterRef.current)
            }
          })

          // Save unlisten function to global state
          globalListenerState.unlisten = unlisten
        } catch (err) {
          console.error(t('dashboard.logs.setupListenerFailed'), err)
          globalListenerState.isActive = false

          // 显示剪贴板监听失败错误
          toast.error(t('dashboard.errors.listenerSetupFailed'), {
            description: err instanceof Error ? err.message : t('dashboard.errors.unknown'),
            duration: 5000,
          })
        }
      } else {
        console.log(t('dashboard.logs.listenerAlreadyActive'))
      }
    }

    // Setup listener if not already set
    if (!globalListenerState.isActive) {
      setupListener()
    } else {
      console.log(t('dashboard.logs.globalListenerExists'))
    }

    // Cleanup function when component unmounts
    return () => {
      // Clear debounce timer
      if (debouncedLoadRef.current) {
        clearTimeout(debouncedLoadRef.current)
      }
      // Don't clean up global listener, keep it active
      console.log(t('dashboard.logs.componentUnmounting'))
    }
  }, [debouncedLoadData, t])

  // Listen for encryption session ready event
  useEffect(() => {
    const setupEncryptionListener = async () => {
      console.log('[Dashboard] Setting up encryption session ready listener')

      try {
        // Listen to encryption://event with type checking
        const unlisten = await listen<'SessionReady' | { type: string }>(
          'encryption://event',
          event => {
            console.log('[Dashboard] Received encryption event:', event.payload)

            const eventType = typeof event.payload === 'string' ? event.payload : event.payload.type

            if (eventType === 'SessionReady') {
              console.log('[Dashboard] Encryption session ready, reloading clipboard data')
              encryptionReadyRef.current = true
              dispatch(setNotReady(false))
              if (pendingInitialLoadRef.current) {
                pendingInitialLoadRef.current = false
              }
              loadData({ specificFilter: currentFilterRef.current, reset: true })
            }
          }
        )

        return unlisten
      } catch (err) {
        console.error('[Dashboard] Failed to setup encryption session listener:', err)
        return undefined
      }
    }

    const unlistenPromise = setupEncryptionListener()

    const checkEncryptionStatus = async () => {
      try {
        const status = await getEncryptionSessionStatus()
        if (!status.initialized || status.session_ready) {
          encryptionReadyRef.current = true
          dispatch(setNotReady(false))
          if (pendingInitialLoadRef.current) {
            pendingInitialLoadRef.current = false
            loadData({ specificFilter: currentFilterRef.current, reset: true })
          }
        } else {
          encryptionReadyRef.current = false
          dispatch(setNotReady(true))
          console.log(
            '[Dashboard] Encryption initialized but session not ready; waiting for unlock'
          )
        }
      } catch (err) {
        console.error('[Dashboard] Failed to check encryption session status:', err)
        encryptionReadyRef.current = true
        dispatch(setNotReady(false))
        if (pendingInitialLoadRef.current) {
          pendingInitialLoadRef.current = false
          loadData({ specificFilter: currentFilterRef.current, reset: true })
        }
      }
    }

    checkEncryptionStatus()

    return () => {
      unlistenPromise.then(unlisten => {
        if (unlisten) {
          unlisten()
        }
      })
    }
  }, [dispatch, loadData])

  const handleLoadMore = useCallback(() => {
    if (encryptionReadyRef.current !== true) return
    void loadData()
  }, [loadData])

  return (
    <div className="flex flex-col h-full relative">
      {/* Top search bar - Hidden in MVP */}
      <Header onFilterChange={handleFilterChange} className="hidden" />

      {/* Lifecycle failure banner */}
      {(lifecycleStatus === 'WatcherFailed' || lifecycleStatus === 'NetworkFailed') && (
        <div className="mx-3 mt-2 mb-1 p-3 rounded-md bg-destructive/10 border border-destructive/20 flex items-center justify-between">
          <span className="text-sm font-medium text-destructive">
            {lifecycleStatus === 'WatcherFailed'
              ? t('lifecycle.watcherFailed')
              : t('lifecycle.networkFailed')}
          </span>
          <button
            onClick={retryLifecycle}
            disabled={retrying}
            className="text-sm px-3 py-1 rounded bg-destructive/20 hover:bg-destructive/30 text-destructive font-medium disabled:opacity-50"
          >
            {retrying ? t('lifecycle.retrying') : t('lifecycle.retry')}
          </button>
        </div>
      )}

      {/* Clipboard content area - use flex-1 to make it take remaining space */}
      <div className="flex-1 overflow-hidden relative">
        <ClipboardContent
          filter={currentFilter}
          searchQuery={searchValue}
          hasMore={hasMore}
          onLoadMore={handleLoadMore}
        />
      </div>
    </div>
  )
}

export default DashboardPage
