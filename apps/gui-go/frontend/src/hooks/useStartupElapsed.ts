import { useEffect, useReducer } from 'react'

interface Clock {
  attempt: string
  base: number
  at: number
  now: number
  running: boolean
}
type Action =
  | { type: 'tick'; now: number }
  | { type: 'sample'; attempt: string; elapsed: number; running: boolean; now: number }

function reduceClock(state: Clock, action: Action): Clock {
  if (action.type === 'tick') return { ...state, now: action.now }
  const current = state.base + (state.running ? action.now - state.at : 0)
  return {
    attempt: action.attempt,
    base: action.attempt === state.attempt ? Math.max(current, action.elapsed) : action.elapsed,
    at: action.now,
    now: action.now,
    running: action.running,
  }
}

// Backend samples synchronize the clock; they do not drive its one-second ticks.
export function useStartupElapsed(attempt: string, elapsed: number, running: boolean): number {
  const [clock, dispatch] = useReducer(reduceClock, null, () => {
    const now = performance.now()
    return { attempt, base: elapsed, at: now, now, running }
  })
  useEffect(() => {
    dispatch({ type: 'sample', attempt, elapsed, running, now: performance.now() })
  }, [attempt, elapsed, running])
  useEffect(() => {
    if (!running) return
    const tick = () => dispatch({ type: 'tick', now: performance.now() })
    const timer = setInterval(tick, 1000)
    document.addEventListener('visibilitychange', tick)
    return () => {
      clearInterval(timer)
      document.removeEventListener('visibilitychange', tick)
    }
  }, [running])
  return Math.floor((clock.base + (clock.running ? clock.now - clock.at : 0)) / 1000)
}
