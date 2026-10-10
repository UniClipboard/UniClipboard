import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import { daemonClient } from '@/api/daemon/client'
import { countSearch, querySearch, type TimeRangePreset } from '@/api/daemon/search'
import { CompositeSearchBar } from '@/components/history/composite-search'
import { buildChips } from '@/components/history/composite-search/composite-search-model'
import { useZeroResultRelaxations } from '@/components/history/composite-search/useZeroResultRelaxations'
import ZeroResultRelaxations from '@/components/history/composite-search/ZeroResultRelaxations'
import { buildLiveSearchModel, liveModelToSearchParams } from '@/hooks/liveSearchModel'
import '@/i18n'
import '@/styles/globals.css'

// Component-level check, NOT the History page: only the search box and the
// zero-result relaxations, wired like HistoryPage (same hook, same count
// fetcher, same param mapping, same zero-result component) against a REAL
// isolated daemon. The full page is covered by `history-full-app.tsx`. Only
// the native session hand-off is stubbed: the test passes a GUI session token
// it obtained from that daemon.
const pageErrors: string[] = []
Object.assign(window, { __ucPageErrors: pageErrors })
window.addEventListener('error', e => pageErrors.push(`error: ${e.message}`))
window.addEventListener('unhandledrejection', e =>
  pageErrors.push(`rejection: ${String(e.reason)}`)
)
const params = new URLSearchParams(location.search)
const baseUrl = params.get('daemon') ?? ''
const sessionToken = params.get('token') ?? ''
Object.defineProperty(window, '__UC_DESKTOP_HOST__', {
  configurable: true,
  value: {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    transformCallback: () => 0,
    unregisterCallback: () => {},
    invoke: async (command: string) => {
      if (command === 'get_daemon_session') {
        return { sessionToken, expiresInSecs: 300, refreshAtSecs: 240 }
      }
      throw new Error(`fixture: native command ${command} is not available`)
    },
  },
})
daemonClient.initialize({ baseUrl, wsUrl: `${baseUrl.replace(/^http/, 'ws')}/ws` })

export default function Fixture() {
  const { t } = useTranslation()
  const inputRef = useRef<HTMLInputElement>(null)
  const [type, setType] = useState<Filter>(Filter.All)
  const [tag, setTag] = useState<string | null>(null)
  const [source, setSource] = useState<string | null>(null)
  const [time, setTime] = useState<TimeRangePreset>('all_time')
  const [extension, setExtension] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [result, setResult] = useState<{ key: string; total: number; previews: string[] } | null>(
    null
  )

  const searchParams = liveModelToSearchParams(
    buildLiveSearchModel({
      query,
      activeFilter: type,
      tagFilter: tag,
      sourceFilter: source,
      extensionFilter: extension,
      timeRange: time,
    })
  )
  const key = JSON.stringify(searchParams)
  useEffect(() => {
    let live = true
    querySearch({ ...searchParams, limit: 50 }).then(r => {
      if (live)
        setResult({
          key,
          total: r.data.total,
          previews: r.data.items.map(i => i.textPreview ?? i.fileNames.join(', ')),
        })
    })
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  const loading = result?.key !== key

  const current = { type, tag, source, time, extension }
  const chips = buildChips({ t, sourceOptions: [], tagOptions: [], current })
  const isSearchActive = query.length > 0 || chips.length > 0
  const relaxations = useZeroResultRelaxations({
    active: isSearchActive && !loading && result?.total === 0,
    chips,
    current,
    query,
    fetchCounts: countSearch,
  })
  const resetHandlers = {
    type: () => setType(Filter.All),
    tag: () => setTag(null),
    source: () => setSource(null),
    time: () => setTime('all_time'),
    extension: () => setExtension(null),
  }

  return (
    <div className="flex min-h-screen flex-col gap-4 bg-background p-6 text-foreground">
      <div className="w-[34rem]">
        <CompositeSearchBar
          contentFilter={type}
          sourceFilter={source}
          tagFilter={tag}
          timeRange={time}
          extensionFilter={extension}
          onContentFilterChange={setType}
          onTagFilterChange={setTag}
          onSourceFilterChange={setSource}
          onTimeRangeChange={setTime}
          onExtensionFilterChange={setExtension}
          onQueryChange={() => {}}
          onQuerySubmit={text => setQuery(text.trim())}
          sourceOptions={[]}
          tagOptions={[]}
          totalCount={result?.total ?? 0}
          inputRef={inputRef}
          fetchCounts={countSearch}
          clearShortcutEnabled={false}
        />
      </div>
      <div data-testid="results" data-loading={loading} data-total={result?.total ?? ''}>
        {result?.previews.map(p => (
          <div key={p} className="text-ui-body">
            {p}
          </div>
        ))}
        {result?.total === 0 && (
          <div className="pt-8 text-center">
            <p className="text-ui-section">{t('clipboard.search.noResultsFiltered')}</p>
            {relaxations && (
              <div className="pt-3">
                <ZeroResultRelaxations
                  relaxations={relaxations}
                  onRemove={dimension => resetHandlers[dimension]()}
                />
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Fixture />)
