// Black-box client E2E: real GPUI process, OS input, HTTP fixture and a dedicated paste receiver.
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, mkdir, rm, access, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const exec = promisify(execFile);
const directory = dirname(fileURLToPath(import.meta.url));
const binary = resolve(process.env.UC_GPUI_E2E_BINARY ?? join(directory, '../../../target/debug/uc-gpui-quick-panel'));
const peekaboo = process.env.PEEKABOO_BIN ?? '/opt/homebrew/bin/peekaboo';
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const digest = text => createHash('sha256').update(text).digest('hex');

async function until(description, predicate, timeout = 10_000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const result = await predicate();
    if (result) return result;
    await delay(60);
  }
  throw new Error(`Timed out: ${description}`);
}
function launch(command, args, env = process.env) {
  const child = spawn(command, args, { env, stdio: ['pipe', 'pipe', 'pipe'] });
  child.output = '';
  child.errors = '';
  child.stdout.setEncoding('utf8').on('data', data => { child.output += data; });
  child.stderr.setEncoding('utf8').on('data', data => { child.errors += data; });
  child.on('error', error => { child.errors += error.message; });
  return child;
}
async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise(resolve => child.once('exit', resolve));
  child.kill('SIGTERM');
  await Promise.race([exited, delay(1500)]);
  if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exited; }
}

// These tests intentionally require a visible, unlocked macOS desktop; failure is not silently skipped.
test('GPUI quick panel native end-to-end', { timeout: 180_000 }, async t => {
  assert.equal(process.platform, 'darwin', 'Native E2E requires macOS');
  await access(binary);
  t.diagnostic(`Binary: ${binary}; built ${(await stat(binary)).mtime.toISOString()}`);
  await access(peekaboo);
  const temporary = await mkdtemp(join(tmpdir(), 'uc-gpui-e2e-'));
  const artifacts = resolve(process.env.UC_GPUI_E2E_ARTIFACTS ?? temporary);
  await mkdir(artifacts, { recursive: true });
  let fixture, target, app;
  let address;
  let restoreBaseline = 0, textBaseline = 0;
  const events = () => target.output.split('\n').filter(Boolean).map(line => JSON.parse(line));
  const state = async () => (await fetch(`${address}/__test/state`)).json();
  const input = async (command, args) => {
    assert.equal(app.exitCode, null, `GPUI exited: ${app.errors}`);
    await exec(peekaboo, [command, ...args, '--pid', String(app.pid), '--no-auto-focus', '--no-remote'], { timeout: 10_000 });
  };
  const key = keys => input('press', [keys]);
  const hotkey = keys => input('hotkey', ['--keys', keys]);
  const text = async value => {
    // Seed ASCII before native paste so the legacy empty-query paste shortcut cannot intercept it.
    await input('type', ['seed']);
    await hotkey('cmd,a');
    await input('paste', ['--text', value]);
  };
  const query = async (expected, after = -1) => until(`query ${JSON.stringify(expected)}`, async () => {
    const result = await state();
    if (result.searches <= after) return false;
    const last = result.lastSearch;
    return last && last.query === expected.query && last.contentType === (expected.type ?? null)
      && JSON.stringify([...last.tags].sort()) === JSON.stringify([...(expected.tags ?? [])].sort()) && result;
  });
  const noPaste = async () => {
    // Wait beyond the panel's 80 ms delayed paste and 300 ms search debounce.
    await delay(400);
    assert.equal((await state()).restores.length, restoreBaseline, 'Selecting a filter must not restore a clipboard entry');
    assert.equal(events().filter(e => e.event === 'text').length, textBaseline, 'Filter selection must not type into the paste target');
  };
  const windowInfo = async () => {
    const { stdout } = await exec(peekaboo, ['list', 'windows', '--pid', String(app.pid), '--no-remote', '--json']);
    return JSON.parse(stdout).data.windows.find(window => window.title === 'UniClipboard History');
  };
  const clickAt = async (x, y) => {
    const window = await windowInfo();
    const [origin] = window.bounds;
    const before = events().length;
    target.stdin.write(`click ${origin[0] + x} ${origin[1] + y} ${app.pid}\n`);
    const click = await until('guarded native click', () => events().slice(before).find(e => e.event === 'click'));
    assert.equal(click.ok, true, 'The test panel must own the click point');
  };
  const fresh = async () => {
    await stop(app);
    const count = events().length;
    target.stdin.write('reset\nfront\n');
    await until('paste target is active', () => events().slice(count).some(e => e.event === 'front' && e.active));
    const before = await state();
    restoreBaseline = before.restores.length;
    textBaseline = events().filter(e => e.event === 'text').length;
    app = launch(binary, [], { ...process.env, UNICLIPBOARD_DAEMON_BASE_URL: address,
      UNICLIPBOARD_DAEMON_TOKEN_PATH: join(directory, 'fixture-token.txt'), UC_GPUI_SHORTCUT: 'ctrl+alt+shift+f12', UC_GPUI_SCALE: '1' });
    await query({ query: '' }, before.searches);
    await until('settings and tags loaded', async () => { const value = await state(); return value.settingsReads > before.settingsReads && value.tagsReads > before.tagsReads; });
  };
  const run = async (name, action) => {
    await t.test(name, async () => {
      await fresh();
      try { await action(); }
      catch (error) {
        const window = await windowInfo().catch(() => null);
        if (window) await exec('/usr/sbin/screencapture', ['-x', '-o', '-l', String(window.window_id), join(artifacts, `${name.replace(/[^a-z0-9]+/gi, '-').toLowerCase()}.png`)]).catch(() => {});
        await writeFile(join(artifacts, 'failure.json'), JSON.stringify({ test: name, state: await state(), targetEvents: events(), appErrors: app.errors }, null, 2));
        throw error;
      }
    });
  };
  try {
    await exec('swiftc', [join(directory, 'paste_target.swift'), '-o', join(temporary, 'paste-target')], { timeout: 60_000 });
    target = launch(join(temporary, 'paste-target'), []);
    await until('paste target startup', () => target.output.includes('"event":"ready"'));
    fixture = launch(process.execPath, [join(directory, 'fixture.mjs')], { ...process.env, UC_GPUI_FIXTURE_PORT: '0', UC_GPUI_IMAGE_FIXTURES: '1', UC_GPUI_FILTER_FIXTURES: '1', UC_GPUI_FIXTURE_THEME: process.env.UC_GPUI_E2E_THEME ?? 'light' });
    address = await until('fixture startup', () => fixture.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0]);

    await run('Tab from an empty search cycles the type filter, and Shift+Tab goes back', async () => {
      await key('tab');
      const cycled = await query({ query: '', type: 'text' });
      await noPaste();
      await hotkey('shift,tab');
      await query({ query: '' }, cycled.searches);
      await noPaste();
    });
    await run('typing a matching label still searches the full text', async () => {
      await text('工作 设计');
      await query({ query: '工作 设计' });
      await noPaste();
    });
    await run('copying selected query text uses the editor rather than restoring history', async () => {
      await text('工作 设计'); await query({ query: '工作 设计' });
      await hotkey('cmd,a'); await hotkey('cmd,c');
      const beforeClipboard = events().length;
      target.stdin.write('clipboard\n');
      const clipboard = await until('copied query text', () => events().slice(beforeClipboard).find(e => e.event === 'clipboard'));
      assert.equal(clipboard.sha256, digest('工作 设计'));
      await noPaste();
    });
    await run('Tab turns a matching word into a filter and keeps the remaining query', async () => {
      await text('工作 设计');
      await query({ query: '工作 设计' });
      await key('tab');
      await query({ query: '设计', tags: ['工作'] });
      await noPaste();
      await text('图片 设计');
      await query({ query: '图片 设计', tags: ['工作'] });
      await key('tab');
      await query({ query: '设计', type: 'image', tags: ['工作'] });
      await noPaste();
    });
    await run('Escape dismisses suggestions without discarding the query', async () => {
      await text('工作 设计'); await query({ query: '工作 设计' });
      await key('escape');
      await input('type', ['x']);
      await query({ query: '工作 设计x' });
      await noPaste();
    });
    await run('clicking a filter chip removes only that condition and returns input focus', async () => {
      await text('工作 设计'); await query({ query: '工作 设计' });
      await key('tab'); await query({ query: '设计', tags: ['工作'] });
      // The chip sits after the search icon in the 48 px search row.
      await clickAt(60, 25);
      await query({ query: '设计' });
      await input('type', ['x']); await query({ query: '设计x' });
      await noPaste();
    });
    await run('pending search blocks pasting stale history results', async () => {
      await text('slow');
      await until('slow search started', async () => (await state()).searchStarts.includes('slow'));
      await key('return');
      await noPaste();
      await query({ query: 'slow' });
    });
    await run('multiple tags select the intersection rather than the first union result', async () => {
      await text('图片 收藏 工作 预览'); await query({ query: '图片 收藏 工作 预览' });
      await key('tab');
      await query({ query: '收藏 工作 预览', type: 'image' });
      await key('tab');
      await query({ query: '工作 预览', type: 'image', tags: ['favorited'] });
      await key('tab');
      await query({ query: '预览', type: 'image', tags: ['favorited', '工作'] });
      await noPaste();
      await hotkey('cmd,c');
      await until('intersection result copied', async () => (await state()).restores.length > restoreBaseline);
      assert.deepEqual((await state()).restores.slice(restoreBaseline), ['image-landscape']);
    });
    await run('plain Enter pastes the history result even when suggestions are visible', async () => {
      await text('工作 设计'); await query({ query: '工作 设计' });
      await delay(120);
      await key('return');
      await until('history entry restored', async () => (await state()).restores.length === restoreBaseline + 1);
      assert.deepEqual((await state()).restores.slice(restoreBaseline), ['fixture-0']);
      const expected = digest('GPUI quick panel paste verification · 工作 设计');
      await until('text pasted into isolated target', () => events().filter(e => e.event === 'text').slice(textBaseline).some(e => e.sha256 === expected));
    });
    await run('Command+digit pastes the row that carries that digit', async () => {
      await hotkey('cmd,2');
      await until('second row restored', async () => (await state()).restores.length === restoreBaseline + 1);
      assert.deepEqual((await state()).restores.slice(restoreBaseline), ['fixture-1']);
    });
    await run('Command+Q does not quit the panel', async () => {
      await hotkey('cmd,q');
      await delay(1000);
      assert.equal(app.exitCode, null, 'A supervised panel must keep running; its GUI decides when it stops');
      assert.equal(app.signalCode, null);
    });
    await run('Option+Backspace edits the search text and Command+Shift+Backspace deletes the entry', async () => {
      await text('设计'); await query({ query: '设计' });
      await hotkey('alt,delete');
      await query({ query: '' });
      await delay(400);
      assert.deepEqual((await state()).deleted, [], 'Deleting a word in the search box must not delete an entry');
      await hotkey('cmd,shift,delete');
      await until('selected entry deleted', async () => (await state()).deleted.length === 1);
      assert.deepEqual((await state()).deleted, ['fixture-0']);
    });
  } finally {
    await stop(app);
    await stop(fixture);
    if (target) {
      target.stdin.end('quit\n');
      await until('paste target exits and restores clipboard', () => target.exitCode !== null || target.signalCode !== null, 3000).catch(() => stop(target));
    }
    const failed = await access(join(artifacts, 'failure.json')).then(() => true, () => false);
    if (failed) t.diagnostic(`Failure artifacts: ${artifacts}`);
    if (artifacts !== temporary || !failed) await rm(temporary, { recursive: true, force: true });
  }
});
