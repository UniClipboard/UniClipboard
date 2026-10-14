// Linux UI-kit E2E flows driven from e2e-driver.ts: the kit Settings page, against the real daemon
// of an isolated synthetic profile. Every step is reported through `ctx.record`; `shot:<name>` steps
// ask the orchestrator (e2e/linux_kit_ui_run.py) for a screenshot. Linux skin only (it queries the `adw-` classes).
import { Call } from '@wailsio/runtime'
import { getSettings } from '@/api/daemon/settings'
import { SETTINGS_CATEGORIES } from '@/components/setting/settings-config'
import { skinClass } from '@/e2e/skin'
import i18n from '@/i18n'

export interface FlowContext {
  record: (step: string, ok: boolean, detail?: unknown) => Promise<unknown>
  control: (action: string) => Promise<unknown>
  waitFor: <T>(label: string, probe: () => T | null | undefined | false, ms?: number) => Promise<T>
  sleep: (ms: number) => Promise<unknown>
  $: (selector: string) => HTMLElement | null
  fill: (selector: string, value: string) => void
  navControl: (href: string) => HTMLElement | null
  mainLayout: () => unknown
  reachMainLayout: () => Promise<void>
}

const USAGE_LABEL = 'settings.sections.general.telemetry.usageAnalytics.label'
const SETUP_PASSPHRASE = 'gui-go-synthetic-passphrase'

const t = (key: string) => i18n.t(key)
const telemetryFields = async () => {
  const { general } = await getSettings()
  return {
    telemetryEnabled: general.telemetryEnabled,
    usageAnalyticsEnabled: general.usageAnalyticsEnabled,
  }
}
const daemonSettle = async (ctx: FlowContext, probe: () => Promise<boolean>, ms = 10000) => {
  for (const end = Date.now() + ms; Date.now() < end; await ctx.sleep(300)) {
    if (await probe()) return true
  }
  return false
}

async function createSpaceInGui(ctx: FlowContext) {
  const create = await ctx.waitFor('setup entry', () => ctx.$('[data-testid="setup-entry-create"]'))
  create.click()
  await ctx.waitFor('device form', () => ctx.$('#device-name'))
  ctx.fill('#device-name', 'settings-fixture')
  ctx.fill('#pass1', SETUP_PASSPHRASE)
  ctx.fill('#pass2', SETUP_PASSPHRASE)
  await ctx.sleep(200)
  ctx.$('[data-testid="setup-initialize-submit"]')!.click()
  await ctx.waitFor(
    'setup complete',
    () => ctx.$('[data-testid="setup-complete-later"]') || ctx.mainLayout()
  )
  ctx.$('[data-testid="setup-complete-later"]')?.click()
  await ctx.waitFor('initialised main layout', ctx.mainLayout, 120000)
}

// ---- Settings ----

const ctx$ = (selector: string) => document.querySelector<HTMLElement>(selector)
const kitRoot = () => ctx$('[data-ui-skin="linux"]')
const categoryButton = (label: string) =>
  Array.from(document.querySelectorAll<HTMLElement>('button')).find(
    b => b.textContent?.trim() === label
  ) ?? null
const scrollRegion = () => ctx$(skinClass('settings-scroll'))
const appliedScheme = () => (document.documentElement.classList.contains('dark') ? 'dark' : 'light')

/** The switches, selects and radio groups the open category shows, with their current values. */
function readControls() {
  const region = scrollRegion()
  const all = <T extends Element>(selector: string) =>
    Array.from(region?.querySelectorAll<T>(selector) ?? [])
  return {
    groups: all<HTMLElement>('legend').map(l => l.textContent?.trim()),
    switches: all<HTMLElement>('[role="switch"]').map(s => ({
      label: s.getAttribute('aria-label'),
      checked: s.getAttribute('aria-checked'),
    })),
    selects: all<HTMLElement>('[role="combobox"]').map(s => ({
      label: s.getAttribute('aria-label'),
      value: s.textContent?.trim(),
    })),
    radios: all<HTMLInputElement>('input[type="radio"], [role="radio"]').length,
    buttons: all<HTMLElement>('button').length,
  }
}

/** Collects what the UI shows as save feedback (toast, status, notice) while `act` runs. */
async function feedbackDuring(ctx: FlowContext, act: () => Promise<void>) {
  const seen: string[] = []
  const selector = '[data-sonner-toast], [role="status"], [role="alert"], .adw-notice'
  const observer = new MutationObserver(records => {
    for (const r of records) {
      for (const node of Array.from(r.addedNodes)) {
        if (!(node instanceof HTMLElement)) continue
        const hit = node.matches(selector) ? node : node.querySelector<HTMLElement>(selector)
        if (hit)
          seen.push(
            `${hit.tagName.toLowerCase()}[role=${hit.getAttribute('role')}] ${hit.textContent?.trim().slice(0, 80)}`
          )
      }
    }
  })
  observer.observe(document.body, { childList: true, subtree: true })
  await act()
  await ctx.sleep(2500)
  observer.disconnect()
  const standing = Array.from(document.querySelectorAll<HTMLElement>(selector))
    .map(el => el.textContent?.trim().slice(0, 80))
    .filter(Boolean)
  return { added: seen, standing }
}

async function openSettings(ctx: FlowContext) {
  ctx.navControl('/settings')!.click()
  await ctx.waitFor(
    'kit settings page',
    () => location.pathname.startsWith('/settings') && scrollRegion()
  )
}

async function openCategory(ctx: FlowContext, id: string) {
  const label = t(`settings.categories.${id}`)
  const button = await ctx.waitFor(`category ${id}`, () => categoryButton(label))
  button.click()
  await ctx.waitFor(`${id} content`, () => scrollRegion()?.getAttribute('aria-label') === label)
  await ctx.sleep(400)
  return { label, button }
}

const usageSwitch = () =>
  scrollRegion()?.querySelector<HTMLElement>(`[role="switch"][aria-label="${t(USAGE_LABEL)}"]`) ??
  null

async function flipUsageSwitch(ctx: FlowContext) {
  const control = await ctx.waitFor('usage analytics switch', usageSwitch)
  const was = control.getAttribute('aria-checked') === 'true'
  const daemonBefore = (await telemetryFields()).usageAnalyticsEnabled
  const feedback = await feedbackDuring(ctx, async () => {
    control.click()
    await ctx.waitFor(
      'switch flipped',
      () => usageSwitch()?.getAttribute('aria-checked') === String(!was)
    )
  })
  const settled = await daemonSettle(
    ctx,
    async () => (await telemetryFields()).usageAnalyticsEnabled === !was
  )
  return {
    was,
    now: usageSwitch()?.getAttribute('aria-checked'),
    daemonBefore,
    daemonAfter: (await telemetryFields()).usageAnalyticsEnabled,
    settled,
    feedback,
  }
}

/**
 * Stage `settings-kit`: the Settings page on the Linux kit. Per theme every category is opened and
 * its controls read, a safe synthetic switch (usage analytics) is written and read back from the
 * daemon, then the page returns to History. The switch is left OFF for `settings-restart`.
 */
export async function runSettingsKitScenario(ctx: FlowContext) {
  const { record, waitFor, sleep } = ctx
  const secret = JSON.parse(String(await Call.ByName('main.EvidenceService.Secret'))) as {
    themes: Array<'light' | 'dark'>
    shotPauseMs: number
  }
  await createSpaceInGui(ctx)
  await ctx.reachMainLayout()
  await record('settings-no-blocking-dialog', !document.querySelector('[role="alertdialog"]'), {
    remaining: document.querySelectorAll('[role="alertdialog"]').length,
  })

  await openSettings(ctx)
  const ids = SETTINGS_CATEGORIES.map(c => c.id)
  const names = ids.map(id => t(`settings.categories.${id}`))
  await sleep(500)
  await record(
    'settings-kit-page',
    !!kitRoot() &&
      location.pathname.startsWith('/settings') &&
      !ctx.$('[data-testid="settings-page-header"]') &&
      names.every(n => !!categoryButton(n)) &&
      !!ctx$(skinClass('split')),
    {
      path: location.pathname,
      skin: kitRoot()?.dataset.uiSkin,
      theme: kitRoot()?.dataset.adwTheme,
      scrollRegion: scrollRegion()?.getAttribute('aria-label'),
      categoriesInSidebar: names.filter(n => !!categoryButton(n)),
      legacyHeaderAbsent: !ctx.$('[data-testid="settings-page-header"]'),
      selectors: `${skinClass('split')}, ${skinClass('settings-scroll')}[role=region], sidebar buttons by label`,
    }
  )
  await record('shot:settings-first-open', true)
  await sleep(secret.shotPauseMs)

  const setTheme = async (theme: 'light' | 'dark' | 'system') => {
    await openCategory(ctx, 'appearance')
    const radio = await waitFor('theme choice', () =>
      ctx.$(`input[name="appearance-theme"][value="${theme}"]`)
    )
    radio.click()
    await waitFor('theme saved', () => (radio as HTMLInputElement).checked)
    if (theme !== 'system') await waitFor(`${theme} applied`, () => appliedScheme() === theme)
    return daemonSettle(ctx, async () => (await getSettings()).general.theme === theme)
  }

  for (const theme of secret.themes) {
    const stored = await setTheme(theme)
    await sleep(500)
    await record(
      `settings-theme-${theme}`,
      stored && appliedScheme() === theme && kitRoot()?.dataset.adwTheme === theme,
      {
        applied: appliedScheme(),
        kitRootTheme: kitRoot()?.dataset.adwTheme,
        daemonTheme: (await getSettings()).general.theme,
      }
    )
    const visited: Array<{
      id: string
      label: string
      ok: boolean
      active: string | null
      controls: ReturnType<typeof readControls>
    }> = []
    for (const id of ids) {
      const { label, button } = await openCategory(ctx, id)
      const controls = readControls()
      const active =
        button.getAttribute('aria-current') ??
        button.getAttribute('aria-selected') ??
        button.getAttribute('data-active')
      visited.push({
        id,
        label,
        ok:
          !!scrollRegion() &&
          (scrollRegion()?.children[0]?.childElementCount ?? 0) > 0 &&
          kitRoot()?.dataset.adwTheme === theme,
        active,
        controls,
      })
      await record(`shot:settings-${theme}-${id}`, true, { label })
      await sleep(secret.shotPauseMs)
    }
    const withControls = visited.filter(
      v => v.controls.switches.length + v.controls.selects.length > 0
    )
    await record(
      `settings-categories-${theme}`,
      visited.length === ids.length && visited.every(v => v.ok),
      {
        visited: visited.map(v => ({
          label: v.label,
          ok: v.ok,
          sidebarActiveAttr: v.active,
          groups: v.controls.groups,
          switches: v.controls.switches,
          selects: v.controls.selects,
          radios: v.controls.radios,
        })),
        categoriesWithSwitchOrSelect: withControls.map(v => v.label),
        categoriesWithoutSwitchOrSelect: visited
          .filter(v => !withControls.includes(v))
          .map(v => v.label),
        selectors:
          '[role=switch][aria-label], [role=combobox], input[type=radio]/[role=radio] inside .adw-settings-scroll',
      }
    )

    // Write-back: flip the switch, read it from the daemon, flip it back (the daemon holds both states).
    await openCategory(ctx, 'general')
    const first = await flipUsageSwitch(ctx)
    await record(
      `settings-writeback-${theme}`,
      first.settled && first.now === String(!first.was) && first.daemonAfter === !first.was,
      { control: `[role=switch][aria-label="${t(USAGE_LABEL)}"]`, ...first }
    )
    await record(`settings-save-feedback-${theme}`, true, {
      observed:
        first.feedback.added.length + first.feedback.standing.length > 0
          ? first.feedback
          : 'none observed',
    })
    const second = await flipUsageSwitch(ctx)
    await record(`settings-restore-${theme}`, second.settled && second.daemonAfter === first.was, {
      ...second,
      expectedDaemon: first.was,
    })
  }

  await setTheme('system')
  // The value that must survive the restart: usage analytics off (the profile default is on).
  await openCategory(ctx, 'general')
  const last = await flipUsageSwitch(ctx)
  await record('settings-left-changed', last.settled && last.daemonAfter === false, { ...last })

  const back =
    categoryButton(t('nav.back')) ??
    Array.from(document.querySelectorAll<HTMLElement>('button')).find(b =>
      b.textContent?.trim().startsWith(t('nav.back'))
    )
  back!.click()
  await waitFor('history again', () => location.pathname.startsWith('/history') && ctx.mainLayout())
  await sleep(800)
  await record(
    'settings-back-history',
    !!kitRoot() &&
      location.pathname.startsWith('/history') &&
      !!ctx$('ol[aria-label], [role="listbox"][aria-multiselectable]'),
    {
      path: location.pathname,
      skin: kitRoot()?.dataset.uiSkin,
      historyMarker: ctx$('[role="listbox"][aria-multiselectable]')
        ? 'listbox'
        : ctx$('ol[aria-label]')
          ? 'getting-started list'
          : null,
      sidebarSettingsEntry: !!ctx.navControl('/settings'),
    }
  )
  await record('settings-complete', true)
  await ctx.control('exit')
}

/** Stage `settings-restart`: after a full quit the changed switch is still off (daemon and UI); restore it. */
export async function runSettingsRestartScenario(ctx: FlowContext) {
  await ctx.reachMainLayout()
  const stored = await daemonSettle(
    ctx,
    async () => (await telemetryFields()).usageAnalyticsEnabled === false,
    15000
  )
  await ctx.record('settings-persisted-daemon', stored, {
    daemon: await telemetryFields(),
  })
  await openSettings(ctx)
  await openCategory(ctx, 'general')
  await ctx.sleep(500)
  const uiState = usageSwitch()?.getAttribute('aria-checked')
  await ctx.record('settings-persisted-ui', uiState === 'false', {
    control: `[role=switch][aria-label="${t(USAGE_LABEL)}"]`,
    ariaChecked: uiState,
    theme: (await getSettings()).general.theme,
  })
  const restore = await flipUsageSwitch(ctx)
  await ctx.record(
    'settings-restored-after-restart',
    restore.settled && restore.daemonAfter === true,
    { ...restore }
  )
  await ctx.record('settings-restart-complete', true)
  await ctx.control('exit')
}
