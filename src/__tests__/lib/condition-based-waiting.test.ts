/**
 * Tests for the condition-based waiting polling utilities.
 *
 * These utilities were defined in
 * .agents/skills/systematic-debugging/condition-based-waiting-example.ts
 * (deleted in this PR) and are tested here to preserve coverage of the
 * polling algorithm's behaviour.
 *
 * The three functions share a common pattern:
 *   - Poll a data source every 10 ms
 *   - Resolve when a condition is satisfied
 *   - Reject with a descriptive timeout error when timeoutMs is exceeded
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// ---------------------------------------------------------------------------
// Minimal interface types mirroring the original imports
// ---------------------------------------------------------------------------

type LaceEventType = string

interface LaceEvent {
  type: LaceEventType
  data?: unknown
}

interface ThreadManager {
  getEvents(threadId: string): LaceEvent[]
}

// ---------------------------------------------------------------------------
// Standalone implementations under test
// (Extracted from the deleted condition-based-waiting-example.ts)
// ---------------------------------------------------------------------------

function waitForEvent(
  threadManager: ThreadManager,
  threadId: string,
  eventType: LaceEventType,
  timeoutMs = 5000,
): Promise<LaceEvent> {
  return new Promise((resolve, reject) => {
    const startTime = Date.now()

    const check = () => {
      const events = threadManager.getEvents(threadId)
      const event = events.find((e) => e.type === eventType)

      if (event) {
        resolve(event)
      } else if (Date.now() - startTime > timeoutMs) {
        reject(new Error(`Timeout waiting for ${eventType} event after ${timeoutMs}ms`))
      } else {
        setTimeout(check, 10)
      }
    }

    check()
  })
}

function waitForEventCount(
  threadManager: ThreadManager,
  threadId: string,
  eventType: LaceEventType,
  count: number,
  timeoutMs = 5000,
): Promise<LaceEvent[]> {
  return new Promise((resolve, reject) => {
    const startTime = Date.now()

    const check = () => {
      const events = threadManager.getEvents(threadId)
      const matchingEvents = events.filter((e) => e.type === eventType)

      if (matchingEvents.length >= count) {
        resolve(matchingEvents)
      } else if (Date.now() - startTime > timeoutMs) {
        reject(
          new Error(
            `Timeout waiting for ${count} ${eventType} events after ${timeoutMs}ms (got ${matchingEvents.length})`,
          ),
        )
      } else {
        setTimeout(check, 10)
      }
    }

    check()
  })
}

function waitForEventMatch(
  threadManager: ThreadManager,
  threadId: string,
  predicate: (event: LaceEvent) => boolean,
  description: string,
  timeoutMs = 5000,
): Promise<LaceEvent> {
  return new Promise((resolve, reject) => {
    const startTime = Date.now()

    const check = () => {
      const events = threadManager.getEvents(threadId)
      const event = events.find(predicate)

      if (event) {
        resolve(event)
      } else if (Date.now() - startTime > timeoutMs) {
        reject(new Error(`Timeout waiting for ${description} after ${timeoutMs}ms`))
      } else {
        setTimeout(check, 10)
      }
    }

    check()
  })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Build a mock ThreadManager whose event list can be updated at runtime. */
function makeMockManager(
  initialEvents: LaceEvent[] = [],
): { manager: ThreadManager; addEvent: (e: LaceEvent) => void } {
  const events: LaceEvent[] = [...initialEvents]
  const manager: ThreadManager = {
    getEvents: (_threadId: string) => [...events],
  }
  return {
    manager,
    addEvent: (e: LaceEvent) => events.push(e),
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('condition-based waiting utilities', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  // -------------------------------------------------------------------------
  // waitForEvent
  // -------------------------------------------------------------------------

  describe('waitForEvent', () => {
    it('resolves immediately when the target event is already present', async () => {
      const { manager } = makeMockManager([{ type: 'TOOL_RESULT', data: { id: 1 } }])

      const promise = waitForEvent(manager, 'thread-1', 'TOOL_RESULT', 100)
      await vi.runAllTimersAsync()

      const event = await promise
      expect(event.type).toBe('TOOL_RESULT')
    })

    it('resolves after the event is added asynchronously', async () => {
      const { manager, addEvent } = makeMockManager()

      const promise = waitForEvent(manager, 'thread-1', 'AGENT_MESSAGE', 500)

      // Advance 30 ms — no event yet
      await vi.advanceTimersByTimeAsync(30)

      // Now deliver the event
      addEvent({ type: 'AGENT_MESSAGE', data: 'hello' })

      await vi.runAllTimersAsync()
      const event = await promise
      expect(event.type).toBe('AGENT_MESSAGE')
    })

    it('rejects with descriptive message when timeout is exceeded', async () => {
      const { manager } = makeMockManager()

      const promise = waitForEvent(manager, 'thread-1', 'MISSING_EVENT', 50)

      await vi.advanceTimersByTimeAsync(200)

      await expect(promise).rejects.toThrow('Timeout waiting for MISSING_EVENT event after 50ms')
    })

    it('returns the first matching event and ignores events of other types', async () => {
      const { manager } = makeMockManager([
        { type: 'TOOL_CALL', data: { id: 'a' } },
        { type: 'TOOL_RESULT', data: { id: 'b' } },
        { type: 'TOOL_RESULT', data: { id: 'c' } },
      ])

      const promise = waitForEvent(manager, 'thread-1', 'TOOL_RESULT', 100)
      await vi.runAllTimersAsync()

      const event = await promise
      expect(event.type).toBe('TOOL_RESULT')
      // Expect the FIRST matching event
      expect((event.data as { id: string }).id).toBe('b')
    })

    it('uses a default timeout of 5000 ms when none is specified', async () => {
      const { manager } = makeMockManager()

      const promise = waitForEvent(manager, 'thread-1', 'NEVER')

      // Just past the default 5000 ms threshold
      await vi.advanceTimersByTimeAsync(5100)

      await expect(promise).rejects.toThrow('5000ms')
    })

    it('does not reject before the timeout elapses', async () => {
      const { manager } = makeMockManager()
      let settled = false

      const promise = waitForEvent(manager, 'thread-1', 'LATE_EVENT', 200)
      promise.then(() => (settled = true)).catch(() => (settled = true))

      await vi.advanceTimersByTimeAsync(150)
      expect(settled).toBe(false)

      await vi.advanceTimersByTimeAsync(100)
      await expect(promise).rejects.toThrow()
    })
  })

  // -------------------------------------------------------------------------
  // waitForEventCount
  // -------------------------------------------------------------------------

  describe('waitForEventCount', () => {
    it('resolves immediately when enough events are already present', async () => {
      const { manager } = makeMockManager([
        { type: 'TOOL_CALL', data: { id: 1 } },
        { type: 'TOOL_CALL', data: { id: 2 } },
      ])

      const promise = waitForEventCount(manager, 'thread-1', 'TOOL_CALL', 2, 100)
      await vi.runAllTimersAsync()

      const events = await promise
      expect(events).toHaveLength(2)
    })

    it('resolves when the required count is reached after polling', async () => {
      const { manager, addEvent } = makeMockManager([{ type: 'TOOL_CALL', data: { id: 1 } }])

      const promise = waitForEventCount(manager, 'thread-1', 'TOOL_CALL', 2, 500)

      await vi.advanceTimersByTimeAsync(20)
      addEvent({ type: 'TOOL_CALL', data: { id: 2 } })

      await vi.runAllTimersAsync()
      const events = await promise
      expect(events.length).toBeGreaterThanOrEqual(2)
    })

    it('rejects with count details when timeout is exceeded', async () => {
      const { manager } = makeMockManager([{ type: 'TOOL_CALL', data: { id: 1 } }])

      const promise = waitForEventCount(manager, 'thread-1', 'TOOL_CALL', 3, 50)

      await vi.advanceTimersByTimeAsync(200)

      await expect(promise).rejects.toThrow(
        'Timeout waiting for 3 TOOL_CALL events after 50ms (got 1)',
      )
    })

    it('resolves when count is exceeded (more events than requested)', async () => {
      const { manager } = makeMockManager([
        { type: 'MSG', data: 1 },
        { type: 'MSG', data: 2 },
        { type: 'MSG', data: 3 },
      ])

      const promise = waitForEventCount(manager, 'thread-1', 'MSG', 2, 100)
      await vi.runAllTimersAsync()

      const events = await promise
      // Should resolve with ALL matching events (>= requested count)
      expect(events.length).toBeGreaterThanOrEqual(2)
    })

    it('filters out events of a different type when counting', async () => {
      const { manager } = makeMockManager([
        { type: 'TOOL_CALL', data: 1 },
        { type: 'OTHER', data: 2 },
        { type: 'TOOL_CALL', data: 3 },
      ])

      const promise = waitForEventCount(manager, 'thread-1', 'TOOL_CALL', 2, 100)
      await vi.runAllTimersAsync()

      const events = await promise
      expect(events.every((e) => e.type === 'TOOL_CALL')).toBe(true)
    })

    it('rejects with zero-count detail when no events arrive', async () => {
      const { manager } = makeMockManager()

      const promise = waitForEventCount(manager, 'thread-1', 'TOOL_CALL', 2, 30)

      await vi.advanceTimersByTimeAsync(100)

      await expect(promise).rejects.toThrow('got 0')
    })
  })

  // -------------------------------------------------------------------------
  // waitForEventMatch
  // -------------------------------------------------------------------------

  describe('waitForEventMatch', () => {
    it('resolves immediately when a matching event is already present', async () => {
      const { manager } = makeMockManager([
        { type: 'TOOL_RESULT', data: { id: 'call_123' } },
      ])

      const promise = waitForEventMatch(
        manager,
        'thread-1',
        (e) => e.type === 'TOOL_RESULT' && (e.data as { id: string }).id === 'call_123',
        'TOOL_RESULT with id=call_123',
        100,
      )
      await vi.runAllTimersAsync()

      const event = await promise
      expect(event.type).toBe('TOOL_RESULT')
      expect((event.data as { id: string }).id).toBe('call_123')
    })

    it('resolves when a matching event is added after polling starts', async () => {
      const { manager, addEvent } = makeMockManager([
        { type: 'TOOL_RESULT', data: { id: 'call_456' } }, // wrong id
      ])

      const promise = waitForEventMatch(
        manager,
        'thread-1',
        (e) => e.type === 'TOOL_RESULT' && (e.data as { id: string }).id === 'call_123',
        'TOOL_RESULT with id=call_123',
        500,
      )

      await vi.advanceTimersByTimeAsync(20)
      addEvent({ type: 'TOOL_RESULT', data: { id: 'call_123' } })

      await vi.runAllTimersAsync()
      const event = await promise
      expect((event.data as { id: string }).id).toBe('call_123')
    })

    it('rejects with the supplied description when timeout is exceeded', async () => {
      const { manager } = makeMockManager()

      const promise = waitForEventMatch(
        manager,
        'thread-1',
        () => false,
        'TOOL_RESULT with id=call_999',
        40,
      )

      await vi.advanceTimersByTimeAsync(200)

      await expect(promise).rejects.toThrow(
        'Timeout waiting for TOOL_RESULT with id=call_999 after 40ms',
      )
    })

    it('skips non-matching events and waits for the predicate to be satisfied', async () => {
      const { manager, addEvent } = makeMockManager([
        { type: 'TOOL_RESULT', data: { id: 'wrong' } },
        { type: 'TOOL_CALL', data: { id: 'call_123' } }, // right id, wrong type
      ])

      const promise = waitForEventMatch(
        manager,
        'thread-1',
        (e) => e.type === 'TOOL_RESULT' && (e.data as { id: string }).id === 'call_123',
        'TOOL_RESULT with id=call_123',
        300,
      )

      await vi.advanceTimersByTimeAsync(20)
      addEvent({ type: 'TOOL_RESULT', data: { id: 'call_123' } })

      await vi.runAllTimersAsync()
      const event = await promise
      expect(event.type).toBe('TOOL_RESULT')
      expect((event.data as { id: string }).id).toBe('call_123')
    })

    it('uses the default 5000 ms timeout when none is provided', async () => {
      const { manager } = makeMockManager()

      const promise = waitForEventMatch(manager, 'thread-1', () => false, 'unreachable')

      await vi.advanceTimersByTimeAsync(5100)

      await expect(promise).rejects.toThrow('5000ms')
    })

    it('returns the first event satisfying the predicate', async () => {
      const { manager } = makeMockManager([
        { type: 'TOOL_RESULT', data: { id: 'a', value: 1 } },
        { type: 'TOOL_RESULT', data: { id: 'b', value: 2 } },
      ])

      const promise = waitForEventMatch(
        manager,
        'thread-1',
        (e) => (e.data as { value: number }).value > 0,
        'any TOOL_RESULT with positive value',
        100,
      )
      await vi.runAllTimersAsync()

      const event = await promise
      // Should be the FIRST event satisfying the condition
      expect((event.data as { id: string }).id).toBe('a')
    })
  })

  // -------------------------------------------------------------------------
  // Cross-cutting: polling interval behaviour
  // -------------------------------------------------------------------------

  describe('polling interval', () => {
    it('polls at approximately 10 ms intervals before the event arrives', async () => {
      const { manager, addEvent } = makeMockManager()
      const callTimes: number[] = []

      const countingManager: ThreadManager = {
        getEvents: (threadId: string) => {
          callTimes.push(Date.now())
          return manager.getEvents(threadId)
        },
      }

      const promise = waitForEvent(countingManager, 'thread-1', 'READY', 500)

      // Advance 55 ms without the event → expect ~5-6 polls (initial + one per 10 ms)
      await vi.advanceTimersByTimeAsync(55)

      addEvent({ type: 'READY' })
      await vi.runAllTimersAsync()
      await promise

      // At 10 ms intervals over 55 ms there should be at least 5 calls (with the initial check)
      expect(callTimes.length).toBeGreaterThanOrEqual(5)
    })
  })
})
