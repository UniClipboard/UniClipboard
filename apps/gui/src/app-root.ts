import ReactDOM from 'react-dom/client'

let root: ReactDOM.Root | undefined

/**
 * The window's single React root. The startup screen and the full app render into the same
 * root so the hand-over is one commit, with no unmount/mount gap between them.
 */
export function getAppRoot(): ReactDOM.Root {
  root ??= ReactDOM.createRoot(document.getElementById('root') as HTMLElement)
  return root
}
