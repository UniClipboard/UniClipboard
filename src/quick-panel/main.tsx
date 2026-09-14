import React from 'react'
import ReactDOM from 'react-dom/client'
import { Provider } from 'react-redux'
import '@/i18n'
import { initializeWebviewContextMenu } from '@/lib/webview-context-menu'
import { initializeWindowTheme } from '@/lib/window-theme'
import { initializeWindowUi } from '@/lib/window-ui'
import '@/lib/wdio-test-bridge'
import { store } from '@/store'
import '@/styles/globals.css'
import '@/quick-panel/quick-panel.css'
import QuickPanelApp from './QuickPanelApp'

initializeWindowUi()
initializeWebviewContextMenu()

void initializeWindowTheme().then(() => {
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <Provider store={store}>
        <QuickPanelApp />
      </Provider>
    </React.StrictMode>
  )
})
