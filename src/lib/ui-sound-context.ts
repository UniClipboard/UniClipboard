import { createLogger } from '@/lib/logger'

const log = createLogger('ui-sound-context')

/**
 * How long the UI audio context may stay running after the last cue started.
 * Cuelume's longest recipe (including its shimmer tail) finishes well within
 * this window.
 */
export const UI_AUDIO_IDLE_SUSPEND_MS = 3000

type AudioContextConstructor = typeof AudioContext

type AudioWindow = Window & {
  AudioContext?: AudioContextConstructor
  webkitAudioContext?: AudioContextConstructor
}

type ManagedContext = {
  context: AudioContext
  idleTimer: ReturnType<typeof setTimeout> | null
  suspendRequested: boolean
}

type InstalledPatch = {
  key: 'AudioContext' | 'webkitAudioContext'
  original: AudioContextConstructor
  patched: AudioContextConstructor
  installCount: number
}

const managedContexts = new Set<ManagedContext>()
let installedPatch: InstalledPatch | null = null

const suspendManaged = (managed: ManagedContext): void => {
  if (managed.idleTimer !== null) {
    clearTimeout(managed.idleTimer)
    managed.idleTimer = null
  }

  if (managed.context.state !== 'running') {
    return
  }

  managed.suspendRequested = true
  managed.context.suspend().then(
    () => {
      managed.suspendRequested = false
    },
    err => {
      managed.suspendRequested = false
      log.warn({ err }, 'Failed to suspend idle UI audio context')
    }
  )
}

const markActive = (managed: ManagedContext): void => {
  if (managed.idleTimer !== null) {
    clearTimeout(managed.idleTimer)
  }

  // A cue that starts while an idle suspend is still settling would otherwise
  // stay frozen until the next cue resumes the context.
  if (managed.suspendRequested) {
    managed.suspendRequested = false
    managed.context.resume().catch(() => {})
  }

  managed.idleTimer = setTimeout(() => {
    managed.idleTimer = null
    suspendManaged(managed)
  }, UI_AUDIO_IDLE_SUSPEND_MS)
}

/**
 * Suspends every UI audio context immediately, e.g. when the user turns
 * interaction sounds off.
 */
export const suspendUiAudioContexts = (): void => {
  for (const managed of managedContexts) {
    suspendManaged(managed)
  }
}

// `initializeUiSound()` is documented as safe to call repeatedly, so the patch
// stays installed until every caller has disposed it.
const retainPatch = (patch: InstalledPatch): (() => void) => {
  patch.installCount += 1
  let disposed = false

  return () => {
    if (disposed) {
      return
    }
    disposed = true
    patch.installCount -= 1
    if (patch.installCount > 0 || installedPatch !== patch) {
      return
    }

    const audioWindow = window as AudioWindow
    // Leave the constructor alone if something else replaced it meanwhile.
    if (audioWindow[patch.key] === patch.patched) {
      audioWindow[patch.key] = patch.original
    }
    installedPatch = null
    // Contexts stay owned by Cuelume, so suspend rather than close them: a
    // closed context would make every later cue fail silently.
    suspendUiAudioContexts()
  }
}

/**
 * Makes the `AudioContext` that Cuelume lazily creates release the audio
 * hardware while no cue is playing.
 *
 * Cuelume keeps one shared context in the `running` state for the lifetime of
 * the page and does not expose it. A running context keeps the WebView's
 * output stream open even while it renders silence. On macOS, WKWebView opens
 * that stream with a 128-frame IO buffer and ignores `latencyHint`, so
 * CoreAudio runs the whole output device at 128 frames for every app. On a
 * busy machine that causes system-wide overloads and crackle (#1715).
 *
 * The replacement constructor suspends the context once it has been idle.
 * Cuelume already resumes a suspended context before rendering, so the next
 * cue wakes it up again. Every Cuelume recipe starts with `createGain()`,
 * which is used here as the activity signal.
 *
 * Returns a disposer. The native constructor is restored once every caller
 * has disposed.
 */
export const installIdleSuspendingAudioContext = (): (() => void) => {
  if (typeof window === 'undefined') {
    return () => {}
  }

  if (installedPatch) {
    return retainPatch(installedPatch)
  }

  const audioWindow = window as AudioWindow
  const key = audioWindow.AudioContext
    ? 'AudioContext'
    : audioWindow.webkitAudioContext
      ? 'webkitAudioContext'
      : null
  if (!key) {
    return () => {}
  }

  const NativeAudioContext = audioWindow[key] as AudioContextConstructor

  class IdleSuspendingAudioContext extends NativeAudioContext {
    readonly #managed: ManagedContext

    constructor(options?: AudioContextOptions) {
      super(options)
      this.#managed = { context: this, idleTimer: null, suspendRequested: false }
      managedContexts.add(this.#managed)
      markActive(this.#managed)
    }

    override createGain(): GainNode {
      markActive(this.#managed)
      return super.createGain()
    }

    override close(): Promise<void> {
      if (this.#managed.idleTimer !== null) {
        clearTimeout(this.#managed.idleTimer)
        this.#managed.idleTimer = null
      }
      managedContexts.delete(this.#managed)
      return super.close()
    }
  }

  const patch: InstalledPatch = {
    key,
    original: NativeAudioContext,
    patched: IdleSuspendingAudioContext,
    installCount: 0,
  }
  audioWindow[key] = IdleSuspendingAudioContext
  installedPatch = patch

  return retainPatch(patch)
}
