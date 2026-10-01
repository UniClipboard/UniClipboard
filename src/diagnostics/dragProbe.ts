// Throwaway diagnostic: shows pointer/mouse/touch events and window drag IPC
// calls on screen so a drag failure can be inspected without devtools.
// Not for merge.
const lines: string[] = []
const counts: Record<string, number> = {}
let box: HTMLPreElement | null = null

const render = () => {
  if (!box) return
  const summary = Object.entries(counts)
    .map(([k, v]) => `${k}=${v}`)
    .join(' ')
  box.textContent = `${summary}\n${lines.slice(-8).join('\n')}`
}

const note = (key: string, detail: string) => {
  counts[key] = (counts[key] ?? 0) + 1
  lines.push(`${key} ${detail}`)
  render()
}

const describe = (event: Event): string => {
  const e = event as PointerEvent
  const target = event.target as Element | null
  const region = target?.closest('[data-tauri-drag-region]')?.getAttribute('data-tauri-drag-region')
  return `type=${e.pointerType ?? '-'} id=${e.pointerId ?? '-'} btn=${e.button} btns=${e.buttons} x=${Math.round(e.clientX)} y=${Math.round(e.clientY)} tgt=${target?.tagName} region=${region}`
}

export function installDragProbe() {
  box = document.createElement('pre')
  box.style.cssText =
    'position:fixed;left:8px;bottom:8px;z-index:2147483647;margin:0;padding:6px;max-width:90vw;font:11px monospace;color:#0f0;background:rgba(0,0,0,.85);pointer-events:none;white-space:pre-wrap'
  document.body.appendChild(box)

  for (const type of ['pointerdown', 'mousedown', 'touchstart', 'pointerup', 'pointercancel']) {
    document.addEventListener(type, e => note(type, describe(e)), true)
  }
  let moves = 0
  document.addEventListener(
    'pointermove',
    e => {
      if ((e as PointerEvent).buttons === 0) return
      moves += 1
      if (moves % 10 === 1) note('pointermove(held)', describe(e))
    },
    true
  )

  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: Function } })
    .__TAURI_INTERNALS__
  const original = internals?.invoke
  if (internals && original) {
    internals.invoke = function (cmd: string, ...rest: unknown[]) {
      const interesting = typeof cmd === 'string' && cmd.startsWith('plugin:window|')
      const result = original.call(this, cmd, ...rest)
      if (interesting && /drag|maximize/.test(cmd)) {
        note('invoke', cmd)
        Promise.resolve(result).then(
          () => note('invoke-ok', cmd),
          err => note('invoke-err', `${cmd} ${String(err)}`)
        )
      }
      return result
    }
  } else {
    note('probe', 'no __TAURI_INTERNALS__.invoke')
  }
  note('probe', 'installed')
}
