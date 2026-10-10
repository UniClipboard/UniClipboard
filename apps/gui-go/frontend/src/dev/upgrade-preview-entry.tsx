import ReactDOM from 'react-dom/client'
import '@/i18n'
import '@/App.css'
import { UpgradePreview } from '@/dev/UpgradePreview'

const root = document.getElementById('root')
if (root) ReactDOM.createRoot(root).render(<UpgradePreview />)
