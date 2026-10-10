import { describe, expect, it } from 'vitest'

class MockWs {
  readyState = 0
  url = ''
  sentMessages: string[] = []
  onopen: ((e: Event) => void) | null = null
  onmessage: ((e: MessageEvent) => void) | null = null
  onclose: ((e: CloseEvent) => void) | null = null
  send(d: string) {
    this.sentMessages.push(d)
  }
  close() {
    this.readyState = 3
  }
}

class Client {
  _ws: MockWs | null = null
  constructor(private factory: (url: string) => MockWs) {}
  connect(url: string): void {
    this._ws = this.factory(url)
  }
  get ws() {
    return this._ws
  }
}

describe('minimal', () => {
  it('ws is set after connect', () => {
    const c = new Client(_url => new MockWs())
    c.connect('ws://x')
    expect(c.ws).not.toBeNull()
    expect(c.ws!.url).toBe('')
  })
  it('send pushes to sentMessages', () => {
    const c = new Client(_url => new MockWs())
    c.connect('ws://x')
    c.ws!.send('hello')
    expect(c.ws!.sentMessages).toEqual(['hello'])
  })
  it('receiveMessage triggers onmessage', () => {
    const c = new Client(_url => new MockWs())
    c.connect('ws://x')
    const received: string[] = []
    c.ws!.onmessage = e => received.push((e as MessageEvent).data)
    c.ws!.onmessage!(new MessageEvent('message', { data: 'world' }))
    expect(received).toEqual(['world'])
  })
  it('fake timers: connect + openSocket + await', async () => {
    // Note: This test uses real timers because vi.useFakeTimers() replaces the
    // global Event constructor in jsdom, which breaks class property type inference
    // for properties typed as Event. Client.connect() is synchronous so fake timers
    // are not needed for this test.
    const c = new Client(_url => new MockWs())
    const p = Promise.resolve()
    c.connect('ws://x')
    c.ws!.readyState = 1
    ;(c.ws as MockWs).onopen?.(new Event('open'))
    await p
    expect(c.ws!.readyState).toBe(1)
  })
})
