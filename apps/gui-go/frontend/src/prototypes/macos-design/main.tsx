import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { subscribeWindowMaterial } from '@/host/window-material'
import { isBoardId } from '@/prototypes/macos-design/board-ids'
import { BoardPage } from '@/prototypes/macos-design/boards'
import { Playground } from '@/prototypes/macos-design/playground'
import type { WindowMaterial } from '@/prototypes/macos-design/window'
import '@/ui-kit/skins/macos/tokens.css'
import '@/ui-kit/skins/macos/macos.css'
import '@/prototypes/macos-design/boards.css'

const params = new URLSearchParams(window.location.search)
const shell = params.get('shell') === 'native' ? 'native' : 'web'
// The host reports the material it applied; a browser never has a native material.
let material: WindowMaterial = 'css-fallback'
subscribeWindowMaterial(applied => {
  if (applied !== 'none') material = `native-${applied}`
})
document.documentElement.dataset.shell = shell
document.documentElement.dataset.windowMaterial = material
const boardId = params.get('board')

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {isBoardId(boardId) ? (
      <BoardPage id={boardId} />
    ) : (
      <Playground params={params} shell={shell} material={material} />
    )}
  </StrictMode>
)
