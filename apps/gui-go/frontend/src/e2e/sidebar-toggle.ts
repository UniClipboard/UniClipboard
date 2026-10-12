// Failure modes: duplicate painted/accessible controls, lost focus, incorrect drawer semantics,
// interrupted transitions, or a retained sidebar subtree accepting focus after collapse.
import { Window } from '@wailsio/runtime'
import i18n from '@/i18n'

type RecordStep = (step: string, ok: boolean, detail?: unknown) => Promise<unknown>
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

export async function runSidebarToggleScenario(
  record: RecordStep,
  control: (action: string) => Promise<unknown>
) {
  const wait = async (probe: () => unknown) => {
    const deadline = Date.now() + 60000
    while (!probe()) {
      if (Date.now() > deadline) throw new Error('sidebar scenario timed out')
      await sleep(100)
    }
  }
  await wait(() => document.querySelector('.mac-split'))
  const button = () => document.querySelector<HTMLButtonElement>('.mac-split-toggle')!
  const sample = () => {
    const sidebar = document.querySelector<HTMLElement>('.mac-split-sidebar')!
    const controls = [...document.querySelectorAll<HTMLButtonElement>('.mac-split-toggle')]
    return {
      count: controls.length,
      accessible: controls.filter(b => !b.closest('[inert], [aria-hidden="true"]')).length,
      expanded: button().getAttribute('aria-expanded'),
      label: button().getAttribute('aria-label'),
      sidebarWidth: sidebar.getBoundingClientRect().width,
      toggleLeft: button().getBoundingClientRect().left,
      inert: sidebar.inert,
    }
  }
  const transition = async (name: string, reverse = false) => {
    const original = button()
    original.focus()
    original.click()
    const frames: Array<
      ReturnType<typeof sample> & { ms: number; same: boolean; focused: boolean }
    > = []
    const start = performance.now()
    let reversed = false
    while (performance.now() - start < 500) {
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()))
      if (reverse && !reversed && performance.now() - start >= 80) {
        button().click()
        reversed = true
      }
      frames.push({
        ...sample(),
        ms: performance.now() - start,
        same: button() === original,
        focused: document.activeElement === original,
      })
    }
    const hidden = button().getAttribute('aria-expanded') === 'false'
    const valid =
      frames.every(f => f.count === 1 && f.accessible === 1 && f.same && f.focused) &&
      button().getAttribute('aria-label') ===
        i18n.t(hidden ? 'history.sidebar.show' : 'history.sidebar.hide')
    await record(name, valid, { frames, final: sample() })
    if (hidden) {
      document.querySelector<HTMLElement>('.mac-split-sidebar button')?.focus()
      await record(
        `${name}-hidden-focus`,
        !document.querySelector('.mac-split-sidebar')!.contains(document.activeElement)
      )
    }
  }
  for (const theme of ['light', 'dark']) {
    await Window.SetSize(1200, 720)
    await wait(() => innerWidth >= 1100)
    if (button().getAttribute('aria-expanded') === 'false') button().click()
    await sleep(400)
    const settings = [...document.querySelectorAll<HTMLButtonElement>('nav button')].find(b =>
      b.textContent?.trim().startsWith(i18n.t('history.kit.settings'))
    )
    settings?.click()
    await wait(() => document.querySelector('[data-testid="settings-page-header"]'))
    const appearance = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
      b => b.textContent?.trim() === i18n.t('settings.categories.appearance')
    )!
    appearance.click()
    await wait(() => document.querySelector(`input[name="appearance-theme"][value="${theme}"]`))
    document
      .querySelector<HTMLInputElement>(`input[name="appearance-theme"][value="${theme}"]`)!
      .click()
    await wait(() => document.documentElement.classList.contains('dark') === (theme === 'dark'))
    history.back()
    await wait(() => document.querySelector('.mac-split'))
    for (const width of [1200, 900]) {
      await Window.SetSize(width, 720)
      await wait(() => Math.abs(innerWidth - width) < 5)
      await sleep(400)
      // Begin expanded in both the standard layout and the compact drawer.
      if (button().getAttribute('aria-expanded') === 'false') button().click()
      await sleep(400)
      await transition(`${theme}-${width}-collapse`)
      await record(`shot:${theme}-${width}-collapsed`, true, sample())
      await sleep(2500)
      await transition(`${theme}-${width}-expand`)
      await record(`shot:${theme}-${width}-expanded`, true, sample())
      await sleep(2500)
      await transition(`${theme}-${width}-reverse`, true)
      // Exercise the shared keyboard shortcut through its DOM event handler.
      button().dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 's',
          code: 'KeyS',
          ctrlKey: true,
          metaKey: true,
          bubbles: true,
          cancelable: true,
        })
      )
      await sleep(400)
      await record(
        `${theme}-${width}-shortcut`,
        button().getAttribute('aria-expanded') === 'false',
        sample()
      )
    }
  }
  await record('sidebar-toggle-complete', true, {
    reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches,
  })
  await control('exit')
}
