import { AnimatePresence } from 'framer-motion'
import { Loader2 } from 'lucide-react'
import type React from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetupFlow } from '@/hooks/useSetupFlow'
import { cn } from '@/lib/utils'
import {
  EntryScreen,
  ImportConfigScreen,
  InitializeSpaceScreen,
  PairingCompleteScreen,
  RedeemInvitationScreen,
  SetupBrandPanel,
  ShowInvitationScreen,
} from '@/pages/setup/screens'

interface SetupPageProps {
  onCompleteSetup?: () => void
}

type SetupFlow = ReturnType<typeof useSetupFlow>

interface SetupScreenProps {
  screen: SetupFlow['screen']
  loading: boolean
  goEntry: SetupFlow['goEntry']
  startCreateSpace: SetupFlow['startCreateSpace']
  startJoinSpace: SetupFlow['startJoinSpace']
  startImportConfig: SetupFlow['startImportConfig']
  initializeSpace: SetupFlow['initializeSpace']
  cancelInvitation: SetupFlow['cancelInvitation']
  redeemInvitation: SetupFlow['redeemInvitation']
  onDone: () => void
}

const SetupScreen: React.FC<SetupScreenProps> = ({
  screen,
  loading,
  goEntry,
  startCreateSpace,
  startJoinSpace,
  startImportConfig,
  initializeSpace,
  cancelInvitation,
  redeemInvitation,
  onDone,
}) => {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.page' })
  switch (screen.kind) {
    case 'loading':
      return (
        <div className="flex h-full w-full items-center justify-center">
          <div className="flex items-center gap-3 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" />
            {t('loadingSetupState')}
          </div>
        </div>
      )
    case 'entry':
      return (
        <EntryScreen
          onCreate={startCreateSpace}
          onJoin={startJoinSpace}
          onImport={startImportConfig}
          loading={loading}
        />
      )
    case 'initialize_space':
      return <InitializeSpaceScreen onSubmit={initializeSpace} onBack={goEntry} loading={loading} />
    case 'import_config':
      return <ImportConfigScreen onBack={goEntry} />
    case 'show_invitation':
      return (
        <ShowInvitationScreen
          code={screen.code}
          expiresAtMs={screen.expiresAtMs}
          onCancel={cancelInvitation}
          loading={loading}
        />
      )
    case 'redeem_invitation':
      return (
        <RedeemInvitationScreen onSubmit={redeemInvitation} onBack={goEntry} loading={loading} />
      )
    case 'pairing_complete':
      return <PairingCompleteScreen role={screen.role} redeem={screen.redeem} onDone={onDone} />
  }
}

export default function SetupPage({ onCompleteSetup }: SetupPageProps = {}) {
  const { isMac } = usePlatform()
  const navigate = useNavigate()
  const {
    screen,
    loading,
    goEntry,
    startCreateSpace,
    startJoinSpace,
    startImportConfig,
    initializeSpace,
    cancelInvitation,
    redeemInvitation,
    finishPairing,
  } = useSetupFlow()

  const handleDone = () => {
    finishPairing()
    onCompleteSetup?.()
    navigate('/', { replace: true })
  }

  const stepKey = screen.kind

  return (
    <div className="grid h-full w-full overflow-hidden bg-background lg:grid-cols-[22rem_1fr]">
      <SetupBrandPanel />

      <main className="relative flex min-h-0 flex-col bg-background">
        {/* Drag strip. On macOS below `lg` (brand rail hidden) the traffic
            lights land here, so leave room; once the rail is shown they move
            onto it and the strip reclaims the space. */}
        <header
          data-tauri-drag-region
          className={cn('flex h-12 shrink-0 items-center pr-4', isMac ? 'pl-20 lg:pl-6' : 'pl-6')}
        />

        <div className="flex min-h-0 flex-1 items-center overflow-y-auto px-8 pb-12 sm:px-14">
          <div className="mx-auto w-full max-w-md">
            <AnimatePresence mode="wait" initial={false}>
              <div key={stepKey} className="w-full">
                <SetupScreen
                  screen={screen}
                  loading={loading}
                  goEntry={goEntry}
                  startCreateSpace={startCreateSpace}
                  startJoinSpace={startJoinSpace}
                  startImportConfig={startImportConfig}
                  initializeSpace={initializeSpace}
                  cancelInvitation={cancelInvitation}
                  redeemInvitation={redeemInvitation}
                  onDone={handleDone}
                />
              </div>
            </AnimatePresence>
          </div>
        </div>
      </main>
    </div>
  )
}
