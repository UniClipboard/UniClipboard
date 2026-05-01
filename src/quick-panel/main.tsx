import React from 'react'
import ReactDOM from 'react-dom/client'
import { Provider } from 'react-redux'
import QuickPanelApp from './QuickPanelApp'
import '@/i18n'
import { initializeWindowUi } from '@/lib/window-ui'
import { store } from '@/store'
import '@/styles/globals.css'

initializeWindowUi()

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <Provider store={store}>
      <QuickPanelApp />
    </Provider>
  </React.StrictMode>
)
