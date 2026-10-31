import { openGuiSession } from '../browser-daemon-session.mjs'
import { runScenarios } from '../history-kit-browser.mjs'
import { test, installSession } from './backend-page.mjs'
import { backendOutput as output, backendPlatform as platform } from './backend-suites.mjs'

test('history-kit: original real-backend assertions', async ({
  app,
  backend: daemon,
  createContext,
}) => {
  const guiSession = () => openGuiSession(daemon)

  let apiToken
  async function api(pathname) {
    apiToken ??= await guiSession()
    const response = await fetch(`${daemon.daemonUrl}${pathname}`, {
      headers: { Authorization: `Session ${apiToken}` },
    })
    return { status: response.status, body: response.ok ? await response.json() : null }
  }
  async function preparePage(page) {
    await installSession(page, daemon, await guiSession())
    await page.goto(`${app.baseUrl}/history.html?platform=${platform}`)
  }
  await runScenarios({
    daemon,
    output,
    platform,
    createContext,
    preparePage,
    api,
    guiSession,
    currentInvitation: async () => {
      const { body } = await api('/v2/setup/state')
      return (body?.data ?? body)?.currentInvitation ?? null
    },
    cdp: false,
  })
})
