import assert from 'node:assert/strict'
import { readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { browser } from '@wdio/globals'
import { choose, screenshot, restartDaemon, visible, click } from '../helpers/conflict-gui.js'
import { connect, request, waitFor, stopProfile } from '../helpers/conflict-userdata.mjs'

describe('真实多配置设备关系', () => {
  it(process.env.CONFLICT_CASE ?? 'apply', async () => {
    const run = JSON.parse(await readFile(process.env.CONFLICT_RUN, 'utf8'))
    const scenario = process.env.CONFLICT_CASE ?? 'apply'
    if (scenario === 'native-keyboard') {
      await browser.d.tauri.switchWindow('main')
      let conn
      await waitFor(
        async () => {
          try {
            conn = await connect(run.profiles[3])
            return true
          } catch {
            return false
          }
        },
        'native test daemon',
        30000
      )
      const { nativeKeyboardCase } = await import('../helpers/conflict-controlled.js')
      await nativeKeyboardCase(browser.d, conn, run)
      return
    }
    const nodes = []
    for (const [index, profile] of run.profiles.entries()) {
      const instance = browser['abcde'[index]]
      await instance.tauri.switchWindow('main')
      await instance.execute(async () => {
        await window.__TAURI_INTERNALS__.invoke('plugin:window|show', {
          label: 'main',
        })
      })
      let conn
      await waitFor(
        async () => {
          try {
            conn = await connect(profile)
            return true
          } catch (error) {
            if (error.code === 'ENOENT' || error.cause?.code === 'ECONNREFUSED') return false
            throw error
          }
        },
        'GUI daemon startup',
        30000
      )
      nodes.push(conn)
      if (!run.expected[index]) continue
      if (!['resolve-restored', 'verify-resolved', 'recover-frozen'].includes(scenario))
        await waitFor(async () => {
          const state = await request(conn, '/member/device-group-choices')
          return (
            state.deviceTrust.devices.filter(d => d.membership === 'active').length ===
            run.expected.filter(Boolean).length
          )
        }, 'restored membership')
      assert.equal(
        (await request(conn, '/member/device-group-choices')).deviceTrust.localDeviceId,
        run.expected[index].deviceTrust.localDeviceId
      )
    }
    const ids = run.expected.map(state => state?.deviceTrust.localDeviceId)
    if (!['resolve-restored', 'verify-resolved', 'recover-frozen'].includes(scenario))
      await waitFor(
        async () => {
          for (const [index, node] of nodes.entries()) {
            if (!run.expected[index]) continue
            const state = await request(node, '/member/device-group-choices')
            if (
              state.deviceTrust.devices.some(
                device =>
                  device.groupRelationship !== 'consistent' || device.syncRelationship !== 'usable'
              )
            )
              return false
          }
          return true
        },
        'baseline membership confirmed on every device',
        150000
      )
    await screenshot(browser.a, run, 'restored')
    const results = []
    if (scenario === 'recover-frozen') {
      for (let index = 0; index < nodes.length; index++)
        ids[index] = (
          await request(nodes[index], '/member/device-group-choices')
        ).deviceTrust.localDeviceId
      const removed = ids[nodes.length === 5 ? 2 : 3]
      const recipientIndex = nodes.length === 5 ? 4 : 1
      const expectedMembers = ids.filter(id => id !== removed).sort()
      const handled = new Set()
      await waitFor(
        async () => {
          for (const index of nodes.length === 5 ? [0, 1, 3, 4] : [0, 1]) {
            const state = await request(nodes[index], '/member/device-group-choices')
            const issue = state.issues[0]
            if (!issue) continue
            assert.ok(
              !handled.has(`${index}:${issue.issueId}`),
              'completed recovery choice reopened'
            )
            const selected = await choose(
              browser['abcde'[index]],
              nodes[index],
              run,
              choice =>
                JSON.stringify([...choice.memberDeviceIds].sort()) ===
                JSON.stringify(expectedMembers),
              `recovered-${'abcde'[index]}-${handled.size}`
            )
            handled.add(`${index}:${selected.before.issues[0].issueId}`)
          }
          const left = await request(nodes[0], '/member/device-group-choices')
          const right = await request(nodes[recipientIndex], '/member/device-group-choices')
          return (
            !left.issues.length &&
            !right.issues.length &&
            left.deviceTrust.devices.some(
              d =>
                d.deviceId === ids[recipientIndex] &&
                d.groupRelationship === 'consistent' &&
                d.syncRelationship === 'usable'
            ) &&
            right.deviceTrust.devices.some(
              d =>
                d.deviceId === ids[0] &&
                d.groupRelationship === 'consistent' &&
                d.syncRelationship === 'usable'
            )
          )
        },
        'retained failed state recovers',
        150000
      )
      const state = await request(nodes[0], '/member/device-group-choices')
      assert.deepEqual(
        state.deviceTrust.devices
          .filter(d => d.membership === 'active')
          .map(d => d.deviceId)
          .sort(),
        expectedMembers
      )
      const marker = 'conflict-e2e-recovered-' + Date.now()
      const sent = await request(nodes[0], '/clipboard/dispatch', 'POST', {
        text: marker,
        peers: [ids[recipientIndex]],
      })
      assert.equal(sent.totalAccepted, 1)
      await waitFor(
        async () => (await browser['abcde'[recipientIndex]].$('body').getText()).includes(marker),
        'recovered recipient sees actual text',
        60000
      )
      const blocked = await request(nodes[0], '/clipboard/dispatch', 'POST', {
        text: 'conflict-e2e-recovered-blocked',
        peers: [removed],
      })
      assert.equal(blocked.totalAccepted, 0)
      assert.equal(blocked.totalDuplicate, 0)
      await screenshot(browser['abcde'[recipientIndex]], run, 'recovered-history-received')
      results.push({
        case: 'retained-failure-recovery',
        outcome: 'passed',
        profiles: nodes.length,
        source: run.recoverySource,
      })
    } else if (scenario === 'prepare-pending') {
      await request(nodes[1], '/pairing/unpair', 'POST', { peerId: ids[2] })
      await (
        await browser.d.$('[data-testid="device-trust-confirm"]')
      ).waitForDisplayed({ timeout: 90000 })
      await screenshot(browser.d, run, 'G07-before-exit')
      const groups = await request(nodes[3], '/member/device-group-choices')
      await writeFile(
        join(run.runDir, 'pending-issue.json'),
        JSON.stringify({ issueId: groups.issues[0].issueId })
      )
      results.push({ case: 'G07-prepare', outcome: 'passed' })
    } else if (scenario === 'resolve-restored') {
      const expected = JSON.parse(await readFile(join(run.runDir, 'pending-issue.json'), 'utf8'))
      const groups = await request(nodes[3], '/member/device-group-choices')
      assert.ok(groups.issues.some(issue => issue.issueId === expected.issueId))
      await screenshot(browser.d, run, 'G07-reopened')
      await choose(
        browser.d,
        nodes[3],
        run,
        choice => choice.choiceId === 'apply',
        'restored-choice'
      )
      results.push({ case: 'G07-resolve', outcome: 'passed' })
    } else if (scenario === 'verify-resolved') {
      const groups = await request(nodes[3], '/member/device-group-choices')
      assert.equal(groups.issues.length, 0)
      assert.equal(
        groups.deviceTrust.devices.find(device => device.deviceId === ids[2])?.membership,
        'removed'
      )
      await browser.d.execute(() => window.dispatchEvent(new Event('focus')))
      await (
        await browser.d.$('[data-testid="device-trust-dialog"]')
      ).waitForExist({ reverse: true, timeout: 30000 })
      await screenshot(browser.d, run, 'G07-resolved-after-process-restart')
      results.push({
        case: 'G07',
        outcome: 'passed',
        verification: 'both GUI and daemon processes restarted twice',
      })
    } else if (scenario === 'controlled') {
      const { controlledCases } = await import('../helpers/conflict-controlled.js')
      await controlledCases(browser.d, nodes[3], run)
    } else if (scenario === 'new-peer') {
      assert.equal(nodes.length, 5)
      try {
        await stopProfile(run.profiles[0], 'SIGSTOP')
        await request(nodes[4], '/settings', 'PUT', {
          general: {
            deviceName: 'Test E remote',
            telemetryEnabled: false,
            usageAnalyticsEnabled: false,
            autoCheckUpdate: false,
          },
        })
        const invitation = await request(nodes[1], '/v2/setup/issue-invitation', 'POST')
        await request(nodes[4], '/v2/setup/redeem', 'POST', {
          code: invitation.code,
          passphrase: run.passphrase,
        })
        await waitFor(
          async () => {
            const state = await request(nodes[4], '/member/device-group-choices')
            return state.deviceTrust.currentJoin?.status === 'active'
          },
          'remote-only fifth device joins',
          150000
        )
        ids[4] = (await request(nodes[4], '/member/device-group-choices')).deviceTrust.localDeviceId
        await browser.e.execute(() => {
          window.__beforeJoinReload = true
        })
        await browser.e.refresh()
        await browser.e.waitUntil(
          async () =>
            browser.e.execute(
              () =>
                !window.__beforeJoinReload &&
                !!document.querySelector('[data-testid="history-preview-motion"]')
            ),
          {
            timeout: 60000,
            timeoutMsg: 'newly joined GUI did not load its durable state',
          }
        )
        await waitFor(
          async () => {
            for (const index of [1, 2, 3, 4]) {
              const state = await request(nodes[index], '/member/device-group-choices')
              if (
                state.issues.length ||
                state.deviceTrust.devices
                  .filter(device => ids.slice(1).includes(device.deviceId))
                  .some(
                    device =>
                      device.groupRelationship !== 'consistent' ||
                      device.syncRelationship !== 'usable'
                  )
              )
                return false
            }
            return true
          },
          'fifth-member admission confirmed before removal',
          150000
        )
        await request(nodes[1], '/pairing/unpair', 'POST', { peerId: ids[2] })
        for (const i of [1, 2, 3, 4]) await stopProfile(run.profiles[i], 'SIGSTOP')
        await stopProfile(run.profiles[0], 'SIGCONT')
        await request(nodes[0], '/pairing/unpair', 'POST', { peerId: ids[3] })
      } finally {
        for (const profile of run.profiles) await stopProfile(profile, 'SIGCONT')
      }
      await waitFor(
        async () => {
          const state = await request(nodes[0], '/member/device-group-choices')
          return state.issues.some(issue =>
            issue.choices.some(choice => choice.members?.some(member => member.deviceId === ids[4]))
          )
        },
        'remote fifth member in candidate',
        150000
      )
      const before = await request(nodes[0], '/member/device-group-choices')
      assert.ok(
        !before.deviceTrust.devices.some(device => device.deviceId === ids[4]),
        'fifth device must not be in the current snapshot'
      )
      await click(browser.a, '[data-testid="device-trust-dialog"] button[aria-expanded="false"]')
      assert.ok(
        (await (await browser.a.$('[data-testid="device-trust-dialog"]')).getText()).includes(
          'Test E remote'
        )
      )
      await screenshot(browser.a, run, 'G04-remote-member')
      const result = await choose(
        browser.a,
        nodes[0],
        run,
        choice => !choice.isCurrentGroup,
        'remote-group'
      )
      assert.ok(
        result.after.deviceTrust.devices.some(
          device => device.deviceId === ids[4] && device.membership === 'active'
        )
      )
      if (await visible(browser.e, '[data-testid="setup-complete-done"]'))
        await click(browser.e, '[data-testid="setup-complete-done"]')
      const handled = new Set([`0:${result.before.issues[0].issueId}`])
      const text = 'conflict-e2e-new-peer-' + Date.now()
      await waitFor(
        async () => {
          for (const [index, node] of nodes.entries()) {
            const state = await request(node, '/member/device-group-choices')
            const issue = state.issues[0]
            if (!issue) continue
            const key = `${index}:${issue.issueId}`
            assert.ok(!handled.has(key), 'a completed issue must not reopen on the same device')
            // Admission and removal can arrive as distinct, ordered choices.
            const choice =
              issue.choices.find(
                candidate =>
                  candidate.memberDeviceIds.includes(ids[4]) &&
                  !candidate.memberDeviceIds.includes(ids[2])
              ) ?? issue.choices.find(candidate => candidate.memberDeviceIds.includes(ids[4]))
            assert.ok(choice, 'each new issue must offer the intended new-member group')
            const selected = await choose(
              browser['abcde'[index]],
              node,
              run,
              candidate => candidate.choiceId === choice.choiceId,
              `new-peer-${'abcde'[index]}-${handled.size}`,
              choice.impact?.localDeviceOutcome === 'removed' || choice.requiresRePairing
            )
            handled.add(`${index}:${selected.before.issues[0].issueId}`)
          }
          const left = await request(nodes[0], '/member/device-group-choices')
          const right = await request(nodes[4], '/member/device-group-choices')
          return (
            left.issues.length === 0 &&
            right.issues.length === 0 &&
            left.deviceTrust.devices.some(
              device =>
                device.deviceId === ids[4] &&
                device.syncRelationship === 'usable' &&
                device.groupRelationship === 'consistent'
            ) &&
            right.deviceTrust.devices.some(
              device =>
                device.deviceId === ids[0] &&
                device.syncRelationship === 'usable' &&
                device.groupRelationship === 'consistent'
            )
          )
        },
        'new peer is confirmed on both sides',
        150000
      )
      const delivery = await request(nodes[0], '/clipboard/dispatch', 'POST', {
        text,
        peers: [ids[4]],
      })
      assert.equal(delivery.totalAccepted, 1)
      await waitFor(
        async () => (await browser.e.$('body').getText()).includes(text),
        'new peer receives text in GUI',
        60000
      )
      const blocked = await request(nodes[0], '/clipboard/dispatch', 'POST', {
        text: 'conflict-e2e-rejected',
        peers: [ids[2]],
      })
      assert.equal(blocked.totalAccepted, 0)
      assert.equal(blocked.totalDuplicate, 0)
      await screenshot(browser.e, run, 'G04-received-history')
      results.push({ case: 'G04', outcome: 'passed' })
    } else if (scenario.startsWith('cross-')) {
      try {
        for (const i of [1, 2, 3, ...(nodes.length === 5 ? [4] : [])])
          await stopProfile(run.profiles[i], 'SIGSTOP')
        await request(nodes[0], '/pairing/unpair', 'POST', { peerId: ids[2] })
        await stopProfile(run.profiles[0], 'SIGSTOP')
        await stopProfile(run.profiles[1], 'SIGCONT')
        await request(nodes[1], '/pairing/unpair', 'POST', { peerId: ids[3] })
      } finally {
        for (const profile of run.profiles) await stopProfile(profile, 'SIGCONT')
      }
      await waitFor(
        async () => {
          const state = await request(nodes[0], '/member/device-group-choices')
          return state.issues.some(issue => issue.reason?.kind === 'different_removals')
        },
        'different removal conflict',
        150000
      )
      const result = await choose(
        browser.a,
        nodes[0],
        run,
        choice => choice.isCurrentGroup === (scenario === 'cross-local'),
        'cross'
      )
      results.push({ case: 'G01', outcome: 'passed' })
      const active = result.after.deviceTrust.devices
        .filter(d => d.membership === 'active')
        .map(d => d.deviceId)
        .sort()
      assert.deepEqual(active, [...result.choice.memberDeviceIds].sort())
      const peerState = await request(nodes[1], '/member/device-group-choices')
      if (peerState.issues.length)
        await choose(
          browser.b,
          nodes[1],
          run,
          choice => JSON.stringify([...choice.memberDeviceIds].sort()) === JSON.stringify(active),
          'matching-peer'
        )
      const text = 'conflict-e2e-branch-' + Date.now()
      await waitFor(
        async () => {
          const left = await request(nodes[0], '/member/device-group-choices')
          const right = await request(nodes[1], '/member/device-group-choices')
          return (
            left.deviceTrust.devices.some(
              device =>
                device.deviceId === ids[1] &&
                device.syncRelationship === 'usable' &&
                device.groupRelationship === 'consistent'
            ) &&
            right.deviceTrust.devices.some(
              device =>
                device.deviceId === ids[0] &&
                device.syncRelationship === 'usable' &&
                device.groupRelationship === 'consistent'
            )
          )
        },
        'selected branch is confirmed on both sides',
        150000
      )
      const delivery = await request(nodes[0], '/clipboard/dispatch', 'POST', {
        text,
        peers: [ids[1]],
      })
      assert.equal(delivery.totalAccepted, 1)
      await waitFor(
        async () => (await browser.b.$('body').getText()).includes(text),
        'selected branch receives text in GUI',
        60000
      )
      const excluded = scenario === 'cross-local' ? ids[2] : ids[3]
      const blocked = await request(nodes[0], '/clipboard/dispatch', 'POST', {
        text: 'conflict-e2e-excluded',
        peers: [excluded],
      })
      assert.equal(blocked.totalAccepted, 0)
      assert.equal(blocked.totalDuplicate, 0)
      await screenshot(browser.b, run, 'G01-received-history')
      const oldIssue = result.before.issues[0].issueId
      nodes[0] = await restartDaemon(browser.a, nodes[0])
      nodes[2] = await restartDaemon(browser.c, nodes[2])
      const afterReplay = await request(nodes[0], '/member/device-group-choices')
      assert.equal(
        afterReplay.issues.length,
        0,
        'resolved conflict must stay resolved after peers restart'
      )
      assert.ok(!afterReplay.issues.some(issue => issue.issueId === oldIssue))
      await screenshot(browser.a, run, 'G05-after-peer-restarts')
      results.push({
        case: 'G05',
        outcome: 'passed',
        verification: 'local and third-party restart after completed conflict',
      })
    } else {
      await request(nodes[1], '/pairing/unpair', 'POST', { peerId: ids[2] })
      const role = scenario === 'local-remove' ? 'c' : 'd'
      const index = role === 'c' ? 2 : 3
      const instance = browser[role]
      if (scenario === 'response-loss') {
        await instance.execute(() => {
          const original = window.fetch
          window.__choicePosts = 0
          window.fetch = async (input, init) => {
            const method = init?.method ?? input.method ?? 'GET'
            const url = typeof input === 'string' ? input : input.url
            const response = await original(input, init)
            if (method === 'POST' && new URL(url).pathname === '/member/device-group-choices') {
              const result = await response.clone().json()
              if (
                !response.ok ||
                !['completed', 'pending', 'already_completed'].includes(result.data?.outcome)
              )
                return response
              window.__choicePosts++
              // The real server commits, but this client loses the response.
              window.fetch = original
              throw new TypeError('Controlled response loss')
            }
            return response
          }
        })
      }
      if (scenario === 'disconnect') {
        const beforeDisconnect = await request(nodes[3], '/member/device-group-choices')
        await (
          await instance.$('[data-testid="device-trust-confirm"]')
        ).waitForDisplayed({ timeout: 60000 })
        await click(instance, '[data-testid="device-trust-choice-apply"]')
        await instance.execute(
          endpoint => {
            window.__networkOriginal = window.fetch
            window.fetch = (input, init) =>
              new URL(typeof input === 'string' ? input : input.url, location.href).pathname ===
              '/member/device-group-choices'
                ? Promise.reject(new TypeError('Controlled connection loss'))
                : window.__networkOriginal(input, init)
            window.dispatchEvent(new Event('focus'))
            window.__WDIO_E2E_NETWORK__.disconnect(endpoint)
          },
          nodes[3].base.replace('http:', 'ws:') + '/ws'
        )
        await (await instance.$('[data-testid="device-trust-error"]')).waitForDisplayed()
        await screenshot(instance, run, 'G06-disconnected')
        await choose(
          browser.a,
          nodes[0],
          run,
          choice => choice.choiceId === 'apply',
          'G06-other-device'
        )
        await instance.execute(() => {
          window.fetch = window.__networkOriginal
          delete window.__networkOriginal
          window.__WDIO_E2E_NETWORK__.restore()
        })
        await waitFor(async () => {
          const group = await request(nodes[3], '/member/device-group-choices')
          return group.revision > beforeDisconnect.revision
        }, 'updated relationships after reconnect')
        await (
          await instance.$('[data-testid="device-trust-error"]')
        ).waitForExist({ reverse: true, timeout: 30000 })
        await screenshot(instance, run, 'G06-reconnected')
      }
      if (scenario === 'restart') {
        await (
          await instance.$('[data-testid="device-trust-dialog"]')
        ).waitForDisplayed({ timeout: 60000 })
        await instance.refresh()
        await (
          await instance.$('[data-testid="device-trust-dialog"]')
        ).waitForDisplayed({ timeout: 60000 })
        nodes[index] = await restartDaemon(instance, nodes[index])
      }
      const keep = scenario === 'keep' || scenario === 'disagreement'
      const result = await choose(
        instance,
        nodes[index],
        run,
        choice => choice.choiceId === (keep ? 'keep' : 'apply'),
        'removal',
        role === 'c'
      )
      if (role === 'c') assert.equal(result.after.deviceTrust.localMembership, 'removed')
      else if (!keep)
        assert.equal(
          result.after.deviceTrust.devices.find(d => d.deviceId === ids[2])?.membership,
          'removed'
        )
      else
        assert.ok(
          result.after.deviceTrust.devices.some(
            d => d.deviceId === ids[2] && d.membership === 'active'
          )
        )
      results.push({
        case: role === 'c' ? 'G03' : scenario === 'restart' ? 'G07' : 'removal',
        outcome: 'passed',
      })
      if (scenario === 'response-loss') {
        assert.equal(await instance.execute(() => window.__choicePosts), 1)
        results.push({
          case: 'G08',
          outcome: 'passed',
          fault: 'real POST response discarded after server returns',
        })
      }
      if (scenario === 'disconnect')
        results.push({
          case: 'G06',
          outcome: 'passed',
          fault: 'client member-query transport disconnected',
        })
      if (scenario === 'disagreement') {
        await choose(
          browser.a,
          nodes[0],
          run,
          choice => choice.choiceId === 'apply',
          'accept-opposite'
        )
        await choose(
          browser.c,
          nodes[2],
          run,
          choice => choice.choiceId === 'keep',
          'keep-local-device'
        )
        const retained = await request(nodes[3], '/member/device-group-choices')
        assert.equal(
          retained.issues.length,
          0,
          'an explicit keep decision must not prompt again for the same removal'
        )
        assert.equal(
          retained.deviceTrust.devices.find(device => device.deviceId === ids[2])?.membership,
          'active'
        )
        assert.equal(
          (await request(nodes[0], '/member/device-group-choices')).deviceTrust.devices.find(
            device => device.deviceId === ids[2]
          )?.membership,
          'removed'
        )
        const marker = 'conflict-e2e-retained-' + Date.now()
        await request(nodes[2], '/clipboard/dispatch', 'POST', {
          text: marker,
          peers: [ids[3]],
        })
        await waitFor(
          async () => (await browser.d.$('body').getText()).includes(marker),
          'retained group text arrives',
          60000
        )
        await screenshot(browser.d, run, 'G02-retained-history')
        results.push({ case: 'G02', outcome: 'passed' })
      }
      if (scenario === 'restart') {
        nodes[index] = await restartDaemon(instance, nodes[index])
        await instance.refresh()
        await waitFor(
          async () => !(await request(nodes[index], '/member/device-group-choices')).issues.length,
          'resolved state survives restart'
        )
        await instance.execute(() => window.dispatchEvent(new Event('focus')))
        assert.equal(await visible(instance, '[data-testid="device-trust-confirm"]'), false)
        await screenshot(instance, run, 'after-restart')
      }
      if (scenario === 'apply') {
        // Bring another retained device onto the selected branch through the GUI.
        await choose(browser.a, nodes[0], run, choice => choice.choiceId === 'apply', 'retained')
        await waitFor(async () => {
          const state = await request(nodes[3], '/member/device-group-choices')
          return (
            state.deviceTrust.devices.find(d => d.deviceId === ids[1])?.syncRelationship ===
            'usable'
          )
        }, 'selected peers may sync')
        const marker = 'conflict-e2e-public-send-' + Date.now()
        const delivery = await request(nodes[1], '/clipboard/dispatch', 'POST', {
          text: marker,
          peers: [ids[3]],
        })
        assert.ok(delivery)
        await waitFor(
          async () => {
            await browser.d.execute(() => window.dispatchEvent(new Event('focus')))
            return (await browser.d.$('body').getText()).includes(marker)
          },
          'received text is visible in target GUI',
          60000
        )
        const blocked = await request(nodes[1], '/clipboard/dispatch', 'POST', {
          text: 'conflict-e2e-blocked',
          peers: [ids[2]],
        }).then(
          value => ({ value }),
          error => ({ status: error.status })
        )
        if (blocked.value) {
          assert.equal(blocked.value.totalAccepted, 0)
          assert.equal(blocked.value.totalDuplicate, 0)
          assert.ok(blocked.value.perTarget.every(target => target.outcome === 'error'))
          const relation = (
            await request(nodes[1], '/member/device-group-choices')
          ).deviceTrust.devices.find(device => device.deviceId === ids[2])
          assert.equal(relation?.syncRelationship, 'removed_peer_device')
        } else assert.ok(blocked.status >= 400)
        await screenshot(browser.d, run, 'received-history')
        results.push({ case: 'sync-allowed-and-removed', outcome: 'passed' })
      }
    }
    await writeFile(
      join(run.runDir, `result-${scenario}.json`),
      JSON.stringify(
        {
          scenario,
          profiles: nodes.length,
          results,
          time: new Date().toISOString(),
        },
        null,
        2
      )
    )
  })
})
