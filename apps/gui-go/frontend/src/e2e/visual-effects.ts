// Real Wails binding/storage acceptance; probe evidence is injected only through
// the e2e-tagged native service, never through production environment overrides.
import { EffectsMode, EffectsPersistence, SystemMotion } from '@host/models'
import type { EffectsSnapshot } from '@host/models'
import { Call } from '@wailsio/runtime'
import { commands } from '@/lib/ipc'
import { visualEffectsStore } from '@/lib/visual-effects-store'

const pause = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))
const record = async (step: string, ok: boolean, detail?: unknown) => {
  await Call.ByName('main.EvidenceService.Record', { window: 'main', step, ok, detail })
  if (!ok) throw new Error(step)
}
const control = (action: string) => Call.ByName('main.EvidenceService.Control', action)
async function synced(value: EffectsSnapshot) {
  const deadline = Date.now() + 5000
  while (visualEffectsStore.getSnapshot().revision < value.revision && Date.now() < deadline)
    await pause(20)
  await record(
    'effects-dom',
    document.documentElement.dataset.ucReduceMotion === String(value.reduceMotion) &&
      document.documentElement.dataset.ucLowEffects === String(value.lowEffects),
    value
  )
  await pause(180)
}

export async function runVisualEffects(phase: string) {
  const deadline = Date.now() + 15000
  while (
    (!visualEffectsStore.getSnapshot().sessionId ||
      visualEffectsStore.getSnapshot().revision < 2) &&
    Date.now() < deadline
  )
    await pause(50)
  let state = await commands.getVisualEffects()
  await record('effects-initial', !!state.sessionId, state)
  if (phase !== 'effects:fresh') {
    const expected =
      phase === 'effects:restart' ? EffectsMode.EffectsModeSmooth : EffectsMode.EffectsModeAuto
    await record(
      'effects-restored',
      state.mode === expected && state.lowEffects && state.reduceMotion,
      state
    )
    const readonly = phase.includes('readonly')
    state = await commands.setVisualEffectsMode(EffectsMode.EffectsModeEffects)
    await record(
      'effects-storage-status',
      state.persistence ===
        (readonly
          ? EffectsPersistence.EffectsPersistenceSessionOnly
          : EffectsPersistence.EffectsPersistenceSaved),
      state
    )
    if (!readonly) await commands.setVisualEffectsMode(EffectsMode.EffectsModeSmooth)
    await record('effects-done', true)
    return control('exit')
  }

  await control('open-updater')
  await pause(1200)
  const allow = SystemMotion.SystemMotionAllow
  const reduce = SystemMotion.SystemMotionReduce
  const unknown = SystemMotion.SystemMotionUnknown
  await Call.ByName('main.EvidenceService.ConfigureVisualEffects', 'linux', 'unknown')
  for (const mode of [
    EffectsMode.EffectsModeAuto,
    EffectsMode.EffectsModeEffects,
    EffectsMode.EffectsModeSmooth,
  ]) {
    await commands.setVisualEffectsMode(mode)
    for (const motion of [allow, reduce, unknown]) {
      state = await commands.reportVisualEffectsEnvironment(state.sessionId, motion)
      await record(
        'effects-policy',
        state.reduceMotion === (mode !== EffectsMode.EffectsModeEffects || motion !== allow) &&
          state.lowEffects === (mode !== EffectsMode.EffectsModeEffects),
        state
      )
      await synced(state)
    }
  }
  await commands.reportVisualEffectsEnvironment(state.sessionId, allow)
  const stale = await commands.reportVisualEffectsEnvironment('old-session', reduce)
  await record('effects-stale-environment', stale.systemMotion === allow, stale)
  await commands.setVisualEffectsMode(EffectsMode.EffectsModeAuto)
  await record(
    'effects-linux-no-sample',
    (await commands.beginVisualEffectsSample(state.sessionId)) === null
  )
  await control('close-updater')

  for (const platform of ['darwin', 'windows']) {
    for (const classification of ['unknown', 'constrained', 'capable']) {
      await Call.ByName('main.EvidenceService.ConfigureVisualEffects', platform, classification)
      state = await commands.getVisualEffects()
      await record(
        'effects-capability-fixture',
        state.autoForSession === (classification === 'capable' ? 'effects' : 'smooth'),
        { platform, classification, state }
      )
    }
  }
  for (let index = 0; index < 3; index++) {
    await Call.ByName('main.EvidenceService.AdvanceVisualEffectsInterval')
    const permit = await commands.beginVisualEffectsSample(state.sessionId)
    await record('effects-permit', permit !== null, permit)
    if (!permit) throw new Error('permit absent')
    await record(
      'effects-rate-limit',
      (await commands.beginVisualEffectsSample(state.sessionId)) === null
    )
    const sample = { ...permit, frames: 30, durationMs: 1800, longFrames: 6, longestMs: 100 }
    await pause(1800)
    state = await commands.reportVisualEffectsSample(sample)
    await record(
      'effects-next-launch-only',
      !state.lowEffects && state.nextAuto === (index === 2 ? 'smooth' : null),
      state
    )
    const duplicate = await commands.reportVisualEffectsSample(sample)
    await record('effects-duplicate-sample', duplicate.revision === state.revision)
  }
  await Call.ByName('main.EvidenceService.ConfigureVisualEffects', 'windows', 'capable')
  state = await commands.getVisualEffects()
  await record(
    'effects-persistent-downgrade',
    state.autoForSession === 'smooth' && state.reason === 'runtime',
    state
  )
  await Call.ByName('main.EvidenceService.SetVisualEffectsWriteFailure', true)
  state = await commands.setVisualEffectsMode(EffectsMode.EffectsModeEffects)
  await record(
    'effects-write-failure-applies',
    !state.lowEffects &&
      !state.reduceMotion &&
      state.persistence === EffectsPersistence.EffectsPersistenceSessionOnly,
    state
  )
  await synced(state)
  await Call.ByName('main.EvidenceService.SetVisualEffectsWriteFailure', false)
  await synced(await commands.setVisualEffectsMode(EffectsMode.EffectsModeSmooth))
  await record('effects-done', true)
  await control('exit')
}

export async function reportSecondaryEffects() {
  const report = async () => {
    await pause(100)
    const state = visualEffectsStore.getSnapshot()
    await Call.ByName('main.EvidenceService.Record', {
      window: 'updater',
      step: 'effects-secondary',
      ok:
        !!state.sessionId &&
        document.documentElement.dataset.ucReduceMotion === String(state.reduceMotion) &&
        document.documentElement.dataset.ucLowEffects === String(state.lowEffects),
      detail: state,
    })
  }
  await report()
  visualEffectsStore.subscribe(() => {
    void report()
  })
}
