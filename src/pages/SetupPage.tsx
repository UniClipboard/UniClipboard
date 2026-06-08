import { AnimatePresence } from 'framer-motion'
import { Loader2 } from 'lucide-react'
import type React from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import FloatingParticles from '@/components/effects/FloatingParticles'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetupFlow } from '@/hooks/useSetupFlow'
import {
  EntryScreen,
  InitializeSpaceScreen,
  PairingCompleteScreen,
  RedeemInvitationScreen,
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
      return <EntryScreen onCreate={startCreateSpace} onJoin={startJoinSpace} loading={loading} />
    case 'initialize_space':
      return <InitializeSpaceScreen onSubmit={initializeSpace} onBack={goEntry} loading={loading} />
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
    <div className="relative h-full w-full overflow-hidden bg-background">
      <div
        data-uc-decorative-effect="true"
        className="pointer-events-none absolute inset-0 overflow-hidden"
      >
        <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/30" />
        <div className="aurora-drift-1 absolute -top-32 -left-32 size-[28rem] rounded-full bg-blue-500/25 blur-[6rem] dark:bg-blue-500/15" />
        <div className="aurora-drift-2 absolute -bottom-24 -right-24 size-[24rem] rounded-full bg-emerald-500/25 blur-[5rem] dark:bg-emerald-500/15" />
        <div className="aurora-drift-3 absolute top-1/3 left-1/2 size-[20rem] -translate-x-1/2 rounded-full bg-violet-500/20 blur-[5rem] dark:bg-violet-500/12" />
        <FloatingParticles />
      </div>

      <div className="relative flex h-full w-full min-h-0 flex-col">
        <header
          data-tauri-drag-region
          className={`relative z-10 flex h-12 shrink-0 items-center pr-4 ${
            isMac ? 'pl-20' : 'pl-4'
          }`}
        />

        <main className="flex min-h-0 flex-1 items-center overflow-y-auto px-8 py-4 sm:px-12 sm:py-6">
          <div className="mx-auto w-full max-w-3xl max-h-full">
            <div className="max-h-full px-1 py-1 sm:px-0 sm:py-2">
              <AnimatePresence mode="wait" initial={false}>
                <div key={stepKey} className="w-full">
                  <SetupScreen
                    screen={screen}
                    loading={loading}
                    goEntry={goEntry}
                    startCreateSpace={startCreateSpace}
                    startJoinSpace={startJoinSpace}
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
    </div>
  )
}
