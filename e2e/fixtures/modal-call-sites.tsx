// Browser-only fixture: renders real modal call sites already open for geometry measurement.
import { createRoot } from 'react-dom/client'
import { MemoryRouter } from 'react-router'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import RebuildSpaceDialog from '@/components/device/RebuildSpaceDialog'
import UnpairAlertDialog from '@/components/device/UnpairAlertDialog'
import { FeedbackDialog } from '@/components/feedback/FeedbackDialog'
import RePairingNotice from '@/components/RePairingNotice'
import ChangePassphraseDialog from '@/components/security/ChangePassphraseDialog'
import ClearHistoryDialog from '@/components/setting/ClearHistoryDialog'
import { PackageManagerUpdateDialog } from '@/components/update/PackageManagerUpdateDialog'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import i18n from '@/i18n'
import { FactoryResetDialog } from '@/pages/unlock/FactoryResetDialog'
import '@/styles/globals.css'

const params = new URLSearchParams(location.search)
await i18n.changeLanguage(params.get('language') || 'en-US')
const site = params.get('site') ?? 'clear-history'
const noop = () => {}

export default function Fixture() {
  switch (site) {
    case 'delete':
      return <DeleteConfirmDialog open onOpenChange={noop} onConfirm={noop} count={3} />
    case 'change-passphrase':
      return <ChangePassphraseDialog open onOpenChange={noop} onChanged={noop} />
    case 'unpair':
      return (
        <UnpairAlertDialog
          open
          onOpenChange={noop}
          deviceName="Marks-MacBook-Pro-with-a-rather-long-name"
          busy={false}
          onConfirm={noop}
        />
      )
    case 'rebuild-space':
      return <RebuildSpaceDialog onClose={noop} />
    case 'feedback':
      return <FeedbackDialog open onOpenChange={noop} />
    case 're-pairing':
      return <RePairingNotice onOpenDevices={noop} onDontShowAgain={noop} />
    case 'package-manager-update':
      return (
        <PackageManagerUpdateDialog open onOpenChange={noop} installKind="deb" updateInfo={null} />
      )
    case 'factory-reset':
      return <FactoryResetDialog open onClose={noop} />
    default:
      return <ClearHistoryDialog open onOpenChange={noop} onConfirm={noop} />
  }
}

createRoot(document.getElementById('root')!).render(
  <MemoryRouter>
    <ShortcutProvider>
      <Fixture />
    </ShortcutProvider>
  </MemoryRouter>
)
