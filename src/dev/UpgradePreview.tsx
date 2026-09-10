import { Moon, Pause, Play, RotateCcw, Sun } from 'lucide-react'
import { useEffect, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import { UpgradeProgressScreen } from '@/components/app/UpgradeProgressScreen'
import {
  makeUpgradePreview,
  previewScenarios,
  type PreviewScenario,
} from '@/dev/upgrade-preview-model'

interface State {
  scenario: PreviewScenario
  seconds: number
  playing: boolean
  dark: boolean
  attempt: number
}
type Action =
  | { type: 'tick' }
  | { type: 'scenario'; scenario: PreviewScenario }
  | { type: 'toggle' }
  | { type: 'theme' }
  | { type: 'reset' }
function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'scenario':
      return {
        ...state,
        scenario: action.scenario,
        seconds: 20,
        playing: false,
        attempt: state.attempt + 1,
      }
    case 'toggle':
      return { ...state, playing: !state.playing }
    case 'theme':
      return { ...state, dark: !state.dark }
    case 'reset':
      return {
        ...state,
        scenario: 'upgrading',
        seconds: 0,
        playing: true,
        attempt: state.attempt + 1,
      }
    case 'tick': {
      if (!state.playing) return state
      const seconds = state.seconds + 0.25
      return {
        ...state,
        seconds,
        scenario: seconds >= 43 ? 'ready' : seconds >= 40 ? 'starting' : state.scenario,
        playing: seconds < 43,
      }
    }
  }
}
function initialState(): State {
  const query = new URLSearchParams(window.location.search)
  const candidate = query.get('scenario')
  return {
    scenario: previewScenarios.find(value => value === candidate) ?? 'upgrading',
    seconds: 20,
    playing: false,
    dark: query.get('theme') === 'dark',
    attempt: 1,
  }
}

export function UpgradePreview() {
  const [state, dispatch] = useReducer(reducer, undefined, initialState)
  const { i18n } = useTranslation()
  useEffect(() => {
    if (!state.playing) return
    const timer = setInterval(() => dispatch({ type: 'tick' }), 250)
    return () => clearInterval(timer)
  }, [state.playing])
  useEffect(() => {
    document.documentElement.classList.toggle('dark', state.dark)
    return () => document.documentElement.classList.remove('dark')
  }, [state.dark])
  const snapshot = {
    ...makeUpgradePreview(state.scenario, state.seconds),
    attempt_id: `development-${state.attempt}`,
  }
  function exportSnapshot() {
    const url = URL.createObjectURL(
      new Blob([JSON.stringify({ simulated: true, snapshot }, null, 2)], {
        type: 'application/json',
      })
    )
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = 'upgrade-preview-diagnostics.json'
    anchor.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
      <nav
        aria-label="Development preview"
        className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-4 py-2 text-xs"
      >
        <span className="mr-auto font-mono text-muted-foreground">DEV / Upgrade</span>
        <select
          aria-label="Scenario"
          value={state.scenario}
          onChange={event =>
            dispatch({ type: 'scenario', scenario: event.target.value as PreviewScenario })
          }
          className="h-8 max-w-40 rounded border border-border bg-background px-2"
        >
          {previewScenarios.map(value => (
            <option key={value} value={value}>
              {value}
            </option>
          ))}
        </select>
        <select
          aria-label="Language"
          value={i18n.language}
          onChange={event => void i18n.changeLanguage(event.target.value)}
          className="h-8 rounded border border-border bg-background px-2"
        >
          <option value="zh-CN">中文</option>
          <option value="en-US">English</option>
        </select>
        <button
          type="button"
          title={state.playing ? 'Pause' : 'Play'}
          aria-label={state.playing ? 'Pause' : 'Play'}
          disabled={['failed', 'protection', 'interrupted', 'ready'].includes(state.scenario)}
          className="flex size-8 items-center justify-center rounded hover:bg-muted"
          onClick={() => dispatch({ type: 'toggle' })}
        >
          {state.playing ? <Pause size={16} /> : <Play size={16} />}
        </button>
        <button
          type="button"
          title="Restart preview"
          aria-label="Restart preview"
          className="flex size-8 items-center justify-center rounded hover:bg-muted"
          onClick={() => dispatch({ type: 'reset' })}
        >
          <RotateCcw size={16} />
        </button>
        <button
          type="button"
          title="Toggle theme"
          aria-label="Toggle theme"
          className="flex size-8 items-center justify-center rounded hover:bg-muted"
          onClick={() => dispatch({ type: 'theme' })}
        >
          {state.dark ? <Sun size={16} /> : <Moon size={16} />}
        </button>
      </nav>
      <UpgradeProgressScreen
        key={state.attempt}
        snapshot={snapshot}
        onRetry={() => dispatch({ type: 'reset' })}
        onExport={exportSnapshot}
      />
    </div>
  )
}
