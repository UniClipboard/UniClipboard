import { useState, useSyncExternalStore } from 'react'
import { boardIds } from '@/prototypes/macos-design/board-ids'
import {
  ClipboardWindow,
  type Generation,
  type WindowMaterial,
} from '@/prototypes/macos-design/window'
import { PortalContainer } from '@/ui-kit/skins/macos/portal'

const darkQuery = window.matchMedia('(prefers-color-scheme: dark)')

function useSystemDark() {
  return useSyncExternalStore(
    onChange => {
      darkQuery.addEventListener('change', onChange)
      return () => darkQuery.removeEventListener('change', onChange)
    },
    () => darkQuery.matches
  )
}

/** The interactive window on a desktop-like backdrop, so glass has something to sample. */
export function Playground({
  params,
  shell,
  material,
}: {
  params: URLSearchParams
  shell: 'web' | 'native'
  material: WindowMaterial
}) {
  const systemDark = useSystemDark()
  const [themeChoice, setThemeChoice] = useState(params.get('theme') ?? 'system')
  const [generation, setGeneration] = useState<Generation>(
    params.get('generation') === '27' ? '27' : '26'
  )
  const [root, setRoot] = useState<HTMLDivElement | null>(null)
  const theme = themeChoice === 'system' ? (systemDark ? 'dark' : 'light') : themeChoice
  return (
    <div
      ref={setRoot}
      className="mac-theme"
      data-mac-theme={theme}
      style={{
        minHeight: '100vh',
        background: shell === 'native' ? 'transparent' : 'var(--wall)',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 16,
        padding: shell === 'native' ? 0 : '16px 0 32px',
        boxSizing: 'border-box',
      }}
    >
      {shell === 'web' && (
        <nav
          aria-label="Prototype controls"
          className="mac-glass"
          style={{
            display: 'flex',
            gap: 12,
            alignItems: 'center',
            padding: '6px 12px',
            borderRadius: 14,
            fontSize: 12,
          }}
        >
          <label>
            Theme{' '}
            <select value={themeChoice} onChange={event => setThemeChoice(event.target.value)}>
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
          <label>
            macOS{' '}
            <select
              value={generation}
              onChange={event => setGeneration(event.target.value as Generation)}
            >
              <option value="26">26 Tahoe</option>
              <option value="27">27 Golden Gate</option>
            </select>
          </label>
          {boardIds.map(id => (
            <a key={id} href={`?board=${id}`} style={{ color: 'var(--link)' }}>
              {id.toUpperCase()}
            </a>
          ))}
        </nav>
      )}
      <PortalContainer value={root}>
        <ClipboardWindow
          key={generation}
          generation={generation}
          width={shell === 'native' ? '100vw' : 1100}
          height={shell === 'native' ? '100vh' : 720}
          shell={shell}
          material={material}
        />
      </PortalContainer>
    </div>
  )
}
