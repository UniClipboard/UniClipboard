#!/usr/bin/env python3
"""Setup, content-lock and key-loss recovery in the REAL Windows WebView2 app, with CDP input only.

Usage: windows_entry_kit_remote.py ssh:<host>|parallels:<vm> [--out DIR] [--work DIR]
       [--arch amd64|arm64]

Imports the history runner's machine, clean-HEAD build, portable Node and isolation helpers.
Requires the same Go/bun/Rust prerequisites and PLAYWRIGHT_MODULE (playwright-core/index.mjs
for Parallels). Commit the product sources before running: both binaries must come from
one clean HEAD. The entry-cdp phase observes the real page and exposes the existing daemon lock API;
the existing default and unlock-wrong phases submit forms themselves, so cannot own input
in this CDP-only scenario. Screenshots are WebView contents, not OS desktop captures.

Each run creates a unique uc-gui-go-* portable directory under --work, launches in the
logged-in desktop session, creates the space through its setup page, stops only processes
whose executable path belongs to that directory, deletes its sole file-keyring entry,
and relaunches the same profile. Before that restart, content-lock runs in the setup
window: lock through the real API, delete the sandbox key, and submit both passphrases.
No CLI initialization, clipboard access or OS input.
Evidence and logs are saved under --out even on failure. Cleanup removes only that run's
sandbox and scheduled tasks; shared build caches and portable Node remain under --work.
Exit 0 requires all three scenarios, all screenshots and sandbox removal to pass.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import time
import uuid
from pathlib import Path

from windows_history_kit_remote import (
    FRONTEND, ROOT, Machine, build_daemon, build_go_half, portable_node,
    sandbox_env, tunnel, wait_http,
)

# This script is also saved in --out as the reproducible CDP driver artifact.
DRIVER = r"""
import assert from 'node:assert/strict'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
const [out, endpoint, scenario, stage = 'complete'] = process.argv.slice(2)
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const secret = 'gui-go-synthetic-passphrase'
const result = scenario === 'content-lock' && stage === 'passphrase'
  ? JSON.parse(await readFile(path.join(out, `${scenario}-assertions.json`), 'utf8'))
  : { scenario, mode: 'native-webview2-cdp', steps: [], passed: false }
result.passed = false
result.stage = stage
let browser, page
await mkdir(out, { recursive: true })
const save = () => writeFile(path.join(out, `${scenario}-assertions.json`), JSON.stringify(result, null, 2) + '\n')
async function step(name, run) {
  try {
    const detail = await run()
    result.steps.push({ name, ok: true, detail })
  } catch (error) {
    const detail = page ? await page.evaluate(() => window.__ucEntrySnapshot()).catch(() => null) : null
    result.steps.push({ name, ok: false, detail, error: String(error) })
    throw error
  } finally { await save() }
}
// waitForFunction treats a returned Promise as truthy, so an async predicate must be polled from here.
async function until(predicate, timeout = 120000) {
  const deadline = Date.now() + timeout
  for (;;) {
    const state = await page.evaluate(() => window.__ucEntrySnapshot())
    if (predicate(state)) return state
    if (Date.now() > deadline) throw new Error(`timed out; last state ${JSON.stringify(state)}`)
    await page.waitForTimeout(500)
  }
}
async function snapshot() {
  const state = await page.evaluate(() => window.__ucEntrySnapshot())
  assert.equal(state.skin, 'windows', 'data-ui-skin must be windows on the real DOM')
  return state
}
async function shot(name) {
  await page.screenshot({ path: path.join(out, `${name}.png`) })
}
async function main() {
  // The History page mounts its skin root lazily after the main layout, so wait for it.
  await until(state => state.main && state.unlocked === true && state.skin === 'windows')
  const state = await snapshot()
  assert.equal(state.main, true)
  assert.equal(state.unlocked, true)
  if (scenario === 'setup') {
    // The telemetry notice is not guaranteed on every first run (the first Windows run showed the
    // main window without it), so dismiss it when it appears and record whether it did.
    const consent = page.getByRole('alertdialog').filter({ hasText: state.telemetryTitle })
    state.telemetryNoticeShown = await consent.waitFor({ state: 'visible', timeout: 8000 }).then(() => true, () => false)
    if (state.telemetryNoticeShown) {
      await consent.getByRole('button', { name: state.telemetryOptOut, exact: true }).click()
      await consent.waitFor({ state: 'hidden' })
    }
  }
  await shot(`${scenario}-main`)
  return state
}
try {
  browser = await chromium.connectOverCDP(endpoint, { timeout: 120000 })
  await step('real-app-target', async () => {
    // Select the main Wails page, never an updater, helper, or DevTools page.
    const deadline = Date.now() + 120000
    while (Date.now() < deadline) {
      const pages = browser.contexts().flatMap(context => context.pages())
      for (const candidate of pages) {
        if (await candidate.evaluate(() => typeof window.__ucEntrySnapshot === 'function').catch(() => false)) {
          page = candidate
          break
        }
      }
      if (page) break
      await new Promise(resolve => setTimeout(resolve, 250))
    }
    assert.ok(page, 'entry-cdp observer not found in native WebView')
    page.setDefaultTimeout(120000)
    return { url: page.url() }
  })
  if (scenario === 'setup') {
    await step('first-screen', async () => {
      await page.locator('[data-testid="setup-entry-create"]').waitFor({ state: 'visible' })
      const state = await snapshot()
      assert.equal(state.setup, true)
      assert.equal(state.main, false)
      await shot('setup-entry')
      return { ...state, state: 'setup' }
    })
    await page.locator('[data-testid="setup-entry-create"]').click()
    await step('initialize-form', async () => {
      await page.locator('#device-name').waitFor({ state: 'visible' })
      await page.locator('#device-name').fill('kit-windows-entry')
      await page.locator('#pass1').fill(secret)
      await page.locator('#pass2').fill(secret)
      const state = await snapshot()
      assert.equal(state.initialize, true)
      assert.equal(await page.locator('#device-name').inputValue(), 'kit-windows-entry')
      assert.equal(await page.locator('#pass1').inputValue(), secret)
      assert.equal(await page.locator('#pass2').inputValue(), secret)
      await shot('initialize-form')
      return state
    })
    await page.locator('[data-testid="setup-initialize-submit"]').click()
    await page.waitForFunction(() => document.querySelector('[data-testid="setup-complete-later"]') ||
      location.pathname.startsWith('/history'), null, { timeout: 120000 })
    const later = page.locator('[data-testid="setup-complete-later"]')
    if (await later.isVisible()) await later.click()
    await step('setup-created-in-gui', main)
  } else if (scenario === 'content-lock') {
    const locked = state => state.unlock && !state.main && state.unlocked === false &&
      !state.recovery && state.skin === 'windows'
    if (stage === 'entry') {
      await step('content-lock-started-unlocked', async () => {
        await until(state => state.main && state.unlocked === true && state.skin === 'windows')
        const state = await snapshot()
        assert.equal(state.unlock, false)
        assert.equal(state.recovery, false)
        return state
      })
      await step('content-lock-locked-screen', async () => {
        await page.evaluate(() => window.__ucEntryLockContent())
        const state = await until(locked)
        assert.equal(state.unlocked, false, 'host contentUnlocked must confirm the daemon lock')
        assert.equal(state.passphrase, false)
        await page.locator('[data-testid="unlock-content"]').waitFor({ state: 'visible' })
        await shot('content-lock-entry')
        return state
      })
    } else {
      assert.equal(stage, 'passphrase')
      assert.equal(result.scenario, 'content-lock')
      assert.ok(result.steps.some(step => step.name === 'content-lock-locked-screen' && step.ok))
      assert.ok(result.steps.some(step => step.name === 'content-lock-keyring-entry-deleted' && step.ok))
      await step('content-lock-passphrase-form', async () => {
        assert.ok(locked(await snapshot()), 'must still be on the content-lock page before clicking Unlock')
        await page.locator('[data-testid="unlock-content"]').click()
        // Unexpected direct unlock or recovery is a finding, never an alternate accepted path.
        const state = await until(state => state.passphrase || state.main || state.unlocked === true || state.recovery)
        assert.ok(locked(state), `unexpected keyring unlock result: ${JSON.stringify(state)}`)
        assert.equal(state.passphrase, true)
        await page.locator('#unlock-passphrase').waitFor({ state: 'visible' })
        return state
      })
      const input = page.locator('#unlock-passphrase')
      await step('content-lock-wrong-passphrase-rejected', async () => {
        await input.fill(`${secret}-wrong`)
        await input.press('Enter')
        const state = await until(state => state.alerts.includes(state.wrongPassphrase) ||
          state.main || state.unlocked === true || state.recovery)
        assert.ok(state.wrongPassphrase.length > 0)
        assert.ok(state.alerts.includes(state.wrongPassphrase), 'exact localized wrongPassphrase alert is required')
        assert.equal(state.passphraseInvalid, 'true')
        assert.ok(locked(state), `wrong passphrase changed lock state: ${JSON.stringify(state)}`)
        assert.equal(state.passphrase, true)
        await shot('content-lock-wrong')
        return state
      })
      await step('content-lock-right-passphrase-unlocked', async () => {
        await input.fill(secret)
        await input.press('Enter')
        const state = await main()
        assert.equal(state.unlock, false)
        assert.equal(state.recovery, false)
        return state
      })
    }
  } else {
    assert.equal(scenario, 'unlock')
    await step('locked-screen', async () => {
      await page.locator('[data-testid="unlock-content"], #unlock-passphrase, #recovery-passphrase').first().waitFor({ state: 'visible' })
      const state = await snapshot()
      assert.ok(state.unlock || state.recovery)
      assert.equal(state.main, false)
      assert.equal(state.unlocked, false)
      await shot('unlock-entry')
      return state
    })
    // Key loss may gate startup on profile recovery, as in unlock_run.py, or show the unlock page.
    const keyring = page.locator('[data-testid="unlock-content"]')
    if (await keyring.isVisible()) await keyring.click()
    const input = page.locator('#unlock-passphrase, #recovery-passphrase')
    await input.waitFor({ state: 'visible' })
    await input.fill(`${secret}-wrong`)
    await input.press('Enter')
    await until(state => state.alerts.includes(state.wrongPassphrase))
    await step('wrong-passphrase-rejected', async () => {
      const state = await snapshot()
      assert.ok(state.wrongPassphrase.length > 0)
      assert.ok(state.alerts.includes(state.wrongPassphrase), 'exact localized refusal is required')
      await shot('unlock-wrong')
      return state
    })
    await step('still-locked-after-wrong-passphrase', async () => {
      const state = await snapshot()
      assert.equal(state.unlocked, false)
      assert.equal(state.main, false)
      assert.ok(state.unlock || state.recovery)
      assert.equal(await input.isVisible(), true)
      return state
    })
    await input.fill(secret)
    await input.press('Enter')
    await step('right-passphrase-unlocked', main)
  }
  result.passed = result.steps.length > 0 && result.steps.every(step => step.ok)
} catch (error) {
  result.error = String(error)
  if (page) await shot(`${scenario}-failure`).catch(() => {})
} finally {
  await save()
  // Disconnect this CDP client; process lifecycle belongs to the Python orchestrator.
  if (browser) await browser.close().catch(() => {})
}
console.log(JSON.stringify(result, null, 2))
process.exitCode = result.passed ? 0 : 1
"""


def stop_sandbox(machine, app):
    """Stop and verify only processes whose executable path is inside our sandbox."""
    machine.ps(f"""
$ErrorActionPreference='Stop'
$owned = @('{app}\\gui-go.exe', '{app}\\uniclipd.exe', '{app}\\uniclip.exe')
Get-Process | Where-Object {{ $_.Path -and $owned -contains $_.Path }} | Stop-Process -Force
$deadline = (Get-Date).AddSeconds(30)
do {{
  $left = @(Get-Process | Where-Object {{ $_.Path -and $owned -contains $_.Path }})
  if ($left.Count -eq 0) {{ exit 0 }}
  Start-Sleep -Milliseconds 250
}} while ((Get-Date) -lt $deadline)
throw 'Sandbox processes did not stop'
""", timeout=60)


def delete_sandbox_keyring_entry(machine, app):
    """Delete only the sandbox's single file-keyring entry, using a literal path."""
    return machine.ps(f"""
$ErrorActionPreference='Stop'
$keys = @(Get-ChildItem '{app}\\data' -Recurse -File -Filter '*.bin' | Where-Object {{ $_.Directory.Name -eq 'keyring' }})
if ($keys.Count -ne 1) {{ throw "Expected exactly one sandbox keyring entry, found $($keys.Count)" }}
$key = $keys[0].FullName
if (-not $key.StartsWith('{app}\\data\\', [StringComparison]::OrdinalIgnoreCase)) {{ throw 'Key outside sandbox' }}
Remove-Item -LiteralPath $key -Force
if (Test-Path -LiteralPath $key) {{ throw 'Key entry still exists' }}
$key.Substring('{app}\\'.Length)
""")


def launch(machine, app, profile, port, task, scenario):
    """Use the history runner's desktop-session launch pattern with isolated WebView data."""
    env = sandbox_env(profile,
        f"$env:UC_GUI_GO_E2E_BROWSER_ARGS='--remote-debugging-port={port}'; "
        "$env:UC_GUI_GO_E2E_PHASE='entry-cdp'; "
        f"$env:WEBVIEW2_USER_DATA_FOLDER='{app}\\webview2'; "
        "$env:UC_GUI_GO_EXIT_MODE='full'; $env:UC_GUI_GO_E2E_VISIBLE='1'; "
        f"$env:UC_GUI_GO_EVIDENCE='{app}\\{scenario}-native.jsonl'; ")
    command = f"& '{app}\\gui-go.exe' > '{app}\\{scenario}-gui.out' 2> '{app}\\{scenario}-gui.err'"
    if machine.kind == 'parallels':
        machine.ps(env + f"""
Start-Process -FilePath powershell.exe -WindowStyle Hidden -WorkingDirectory '{app}' -ArgumentList '-NoProfile','-Command',"{command}"
""", timeout=120)
    else:
        machine.ps(f"""
$ErrorActionPreference='Stop'
Set-Content -Path '{app}\\{scenario}-launch.ps1' -Value @'
{env}; Set-Location '{app}'; {command}
'@
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "{app}\\{scenario}-launch.ps1"'
$principal = New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive
Register-ScheduledTask -TaskName '{task}' -Action $action -Principal $principal | Out-Null
Start-ScheduledTask -TaskName '{task}'
""", timeout=120)


def run_driver(machine, app, out, driver, scenario, port, node, playwright, stage='complete'):
    verdict = out / f'{scenario}-assertions.json'
    log_name = scenario if stage == 'complete' else f'{scenario}-{stage}'
    # Only the passphrase stage resumes the verdict written earlier in this invocation.
    if stage != 'passphrase':
        verdict.unlink(missing_ok=True)
    if machine.kind == 'parallels':
        output = machine.ps(f"""
$env:PLAYWRIGHT_MODULE='file:///{playwright.replace(chr(92), '/')}'
$deadline=(Get-Date).AddMinutes(3)
$ready=$false
while ((Get-Date) -lt $deadline) {{
  try {{ Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:{port}/json/version' | Out-Null; $ready=$true; break }} catch {{ Start-Sleep 1 }}
}}
if (-not $ready) {{ throw 'WebView2 CDP did not start' }}
& '{node}' '{machine.share(driver)}' '{machine.share(out)}' 'http://127.0.0.1:{port}' '{scenario}' '{stage}' *>&1 | Out-String
exit $LASTEXITCODE
""", check=False, timeout=600)
        (out / f'{log_name}-driver.log').write_text(output)
    else:
        local, proc = tunnel(machine.name, port)
        try:
            wait_http(f'http://127.0.0.1:{local}/json/version', timeout=180)
            with (out / f'{log_name}-driver.log').open('w') as log:
                subprocess.run(['node', str(driver), str(out), f'http://127.0.0.1:{local}', scenario, stage],
                               cwd=FRONTEND, env=os.environ.copy(), stdout=log, stderr=subprocess.STDOUT,
                               check=True, timeout=600)
        finally:
            proc.terminate()
            proc.wait(timeout=15)
    if not verdict.exists():
        raise RuntimeError(f'{scenario}: CDP driver produced no verdict; see driver log')
    result = json.loads(verdict.read_text(encoding='utf-8-sig'))
    required = {
        'setup': {'real-app-target', 'first-screen', 'initialize-form', 'setup-created-in-gui'},
        'unlock': {'real-app-target', 'locked-screen', 'wrong-passphrase-rejected',
                   'still-locked-after-wrong-passphrase', 'right-passphrase-unlocked'},
        'content-lock': {'real-app-target', 'content-lock-started-unlocked', 'content-lock-locked-screen'},
    }[scenario]
    if scenario == 'content-lock' and stage == 'passphrase':
        required |= {'content-lock-keyring-entry-deleted', 'content-lock-passphrase-form',
                     'content-lock-wrong-passphrase-rejected', 'content-lock-right-passphrase-unlocked'}
    steps = result.get('steps', [])
    if result.get('stage') != stage or not result.get('passed') or not required.issubset({step['name'] for step in steps}) or not all(step['ok'] for step in steps):
        raise RuntimeError(f'{scenario}: failed or missing required assertions; see {verdict.name}')
    shots = ['setup-entry', 'initialize-form', 'setup-main'] if scenario == 'setup' else ['unlock-entry', 'unlock-wrong', 'unlock-main']
    if scenario == 'content-lock':
        shots = ['content-lock-entry'] if stage == 'entry' else [
            'content-lock-entry', 'content-lock-wrong', 'content-lock-main']
    for name in shots:
        if not (out / f'{name}.png').is_file() or (out / f'{name}.png').stat().st_size == 0:
            raise RuntimeError(f'{scenario}: missing screenshot {name}.png')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('machine', help='ssh:<host> or parallels:<vm>')
    parser.add_argument('--out', type=Path, help='local evidence directory')
    parser.add_argument('--work', default=r'C:\uc-entry-kit', help='remote build cache and sandbox parent')
    parser.add_argument('--arch', default='amd64', choices=['amd64', 'arm64'])
    args = parser.parse_args()
    # These paths are interpolated in PowerShell, including a nested launch command.
    # Accept ordinary absolute drive paths only, without shell metacharacters or traversal.
    if not re.fullmatch(r'[A-Za-z]:\\[A-Za-z0-9_ .\\-]+', args.work) or any(
            part in ('.', '..') for part in args.work.split('\\')):
        parser.error('--work must be an absolute Windows drive path without traversal or shell metacharacters')
    machine = Machine(args.machine)
    stamp = time.strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:12]
    out = (args.out or ROOT / f'target/e2e/windows-entry-kit/{stamp}').resolve()
    out.mkdir(parents=True, exist_ok=True)
    driver = out / 'entry-driver.mjs'
    driver.write_text(DRIVER)
    app = args.work.rstrip('\\') + f'\\uc-gui-go-entry-{stamp}'
    profile = f'gui-go-entry-{stamp}'
    record = {'machine': args.machine, 'arch': args.arch, 'sandbox': app, 'profile': profile,
              'systemClipboardDisabled': True, 'input': 'CDP only', 'passed': False, 'scenarios': {}}
    tasks = []
    created = False
    remote_evidence = out / f'remote-evidence-{stamp}'
    try:
        head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        record['head'] = head
        go_out, manifest = build_go_half(args.arch)
        record['goHalf'] = manifest
        daemon, digest, notes = build_daemon(machine, args.work, head, args.arch, out)
        record.update(uniclipdSha256=digest, sourceExtractNotes=notes)
        record['os'] = machine.ps('Get-CimInstance Win32_OperatingSystem | Select Caption,Version,OSArchitecture | ConvertTo-Json -Compress')
        node = playwright = None
        if machine.kind == 'parallels':
            node, playwright = portable_node(machine, args.work, args.arch, out)
        machine.ps(f"$ErrorActionPreference='Stop'; New-Item -ItemType Directory '{app}' | Out-Null")
        created = True
        machine.ps(f"$ErrorActionPreference='Stop'; Copy-Item '{daemon}' '{app}\\uniclipd.exe'")
        for name in ('gui-go.exe', 'uniclip.exe'):
            machine.put(go_out / name, f'{app}\\{name}')
        for scenario in ('setup', 'content-lock', 'unlock'):
            if scenario == 'content-lock':
                entry = run_driver(machine, app, out, driver, scenario, port, node, playwright, 'entry')
                entry['passed'] = False
                deletion = {'name': 'content-lock-keyring-entry-deleted', 'ok': False,
                            'detail': {'lockedSnapshot': entry['steps'][-1]['detail']}}
                try:
                    deleted = delete_sandbox_keyring_entry(machine, app)
                    deletion['detail']['sandboxRelativePath'] = deleted.strip()
                    deletion['ok'] = True
                except Exception as error:
                    deletion['error'] = str(error)
                    raise
                finally:
                    entry['steps'].append(deletion)
                    (out / 'content-lock-assertions.json').write_text(json.dumps(entry, indent=2) + '\n')
                record['scenarios'][scenario] = run_driver(
                    machine, app, out, driver, scenario, port, node, playwright, 'passphrase')
                continue
            if scenario == 'unlock':
                stop_sandbox(machine, app)
                record['keyringEntryDeleted'] = delete_sandbox_keyring_entry(machine, app)
            port = 9300 + int(time.time()) % 500
            task = f'uc-entry-kit-{stamp}-{scenario}'
            if machine.kind == 'ssh':
                tasks.append(task)
            launch(machine, app, profile, port, task, scenario)
            record['scenarios'][scenario] = run_driver(machine, app, out, driver, scenario, port, node, playwright)
        record['passed'] = True
    except (Exception, SystemExit) as error:
        record['error'] = str(error)
    finally:
        cleanup_errors = []
        if created:
            try:
                stop_sandbox(machine, app)
            except Exception as error:
                cleanup_errors.append(f'stop: {error}')
            for task in tasks:
                try:
                    machine.ps(f"Unregister-ScheduledTask -TaskName '{task}' -Confirm:$false -ErrorAction Stop")
                except Exception as error:
                    cleanup_errors.append(f'task {task}: {error}')
            # Preserve every launch's output and portable logs before removing the sandbox.
            try:
                machine.ps(f"""
$ErrorActionPreference='Stop'
New-Item -ItemType Directory -Force '{app}\\evidence' | Out-Null
Get-ChildItem '{app}' -File | Where-Object {{ $_.Extension -in '.out','.err','.jsonl' }} | Copy-Item -Destination '{app}\\evidence'
if (Test-Path '{app}\\data') {{
  Get-ChildItem '{app}\\data' -Recurse -Directory -Filter logs | ForEach-Object {{ Copy-Item -Recurse $_.FullName '{app}\\evidence\\logs' }}
}}
""")
                machine.fetch_dir(app + '\\evidence', remote_evidence)
                # fetch_dir deliberately ignores transport errors; check the required launch logs locally.
                for scenario in record['scenarios']:
                    if scenario == 'content-lock':
                        continue  # This scenario uses the setup launch without a restart.
                    for extension in ('out', 'err'):
                        if not list(remote_evidence.rglob(f'{scenario}-gui.{extension}')):
                            raise RuntimeError(f'missing fetched {scenario} GUI {extension} log')
                if record['scenarios'] and not any(
                        path.is_file() and path.stat().st_size > 0
                        for path in remote_evidence.rglob('uniclipboard-*.json.*')):
                    raise RuntimeError('missing fetched portable application logs')
                record['remoteEvidence'] = str(remote_evidence.relative_to(out))
            except Exception as error:
                cleanup_errors.append(f'evidence: {error}')
            try:
                record['sandboxRemoved'] = machine.ps(f"""
$ErrorActionPreference='Stop'
Set-Location C:\\
Remove-Item -LiteralPath '{app}' -Recurse -Force
if (Test-Path -LiteralPath '{app}') {{ throw 'Sandbox still exists' }}
'REMOVED'
""").strip().endswith('REMOVED')
            except Exception as error:
                cleanup_errors.append(f'remove: {error}')
        record['cleanupErrors'] = cleanup_errors
        record['passed'] = record['passed'] and bool(record.get('sandboxRemoved')) and not cleanup_errors
        (out / 'assertions.json').write_text(json.dumps(record, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({'passed': record['passed'], 'scenarios': list(record['scenarios']),
                      'out': str(out), 'sandboxRemoved': record.get('sandboxRemoved', False),
                      'error': record.get('error'), 'cleanupErrors': record['cleanupErrors']}, indent=2))
    return 0 if record['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
