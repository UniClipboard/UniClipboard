import { Eye, EyeOff, Loader2, Lock, Unlock } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isFactoryResetError,
  isUnlockSpaceError,
  resetSpace,
  unlockEncryptionSession,
  unlockSpaceWithPassphrase,
  verifyKeychainAccess,
  type FactoryResetError,
  type UnlockSpaceError,
} from '@/api/security'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('unlock-page')

/** 二次确认对话框里要求用户打字输入的 sentinel,大小写敏感。 */
const FACTORY_RESET_CONFIRMATION_TOKEN = 'RESET'

interface UnlockPageProps {
  onUnlockSucceeded?: () => void
  /**
   * Factory reset 完成后由 parent 把本地 encryption status 缓存置为
   * `{ initialized: false, session_ready: false }`,从而让 `App.tsx` 的
   * 渲染分支立即切回 `SetupPage`,避免等待 RTK Query 回流的短暂闪烁。
   */
  onResetSucceeded?: () => void
}

function factoryResetErrorI18nKey(error: FactoryResetError): string {
  switch (error.code) {
    case 'KEY_MATERIAL_WIPE_FAILED':
      return 'unlock.factoryReset.errors.keyMaterialWipeFailed'
    case 'STORAGE_FAILED':
      return 'unlock.factoryReset.errors.storageFailed'
    case 'FACADE_UNAVAILABLE':
      return 'unlock.factoryReset.errors.facadeUnavailable'
    case 'INTERNAL':
      return 'unlock.factoryReset.errors.internal'
  }
}

/**
 * 把 `UnlockSpaceError` 翻成 i18n key——按 error.code 选展示文案,WRONG_PASSPHRASE
 * 是用户最常见的可恢复错误,其他都是不可恢复需要引导用户做别的操作。
 */
function unlockErrorI18nKey(error: UnlockSpaceError): string {
  switch (error.code) {
    case 'WRONG_PASSPHRASE':
      return 'unlock.errors.wrongPassphrase'
    case 'CORRUPTED_KEY_MATERIAL':
      return 'unlock.errors.corruptedKeyMaterial'
    case 'SETUP_NOT_COMPLETED':
      return 'unlock.errors.setupNotCompleted'
    case 'SPACE_NOT_INITIALIZED':
      return 'unlock.errors.spaceNotInitialized'
    case 'FACADE_UNAVAILABLE':
      return 'unlock.errors.facadeUnavailable'
    case 'INTERNAL':
      return 'unlock.errors.internal'
  }
}

export default function UnlockPage({ onUnlockSucceeded, onResetSucceeded }: UnlockPageProps) {
  const { t } = useTranslation()
  const { setting, updateSecuritySetting, loading: settingsLoading } = useSetting()
  const { isMac } = usePlatform()

  // ── Silent unlock (top-level button) state ─────────────────────────────
  const [unlocking, setUnlocking] = useState(false)

  // ── Passphrase modal state — only opened when silent unlock can't recover ──
  const [showPassphraseModal, setShowPassphraseModal] = useState(false)
  const [passphrase, setPassphrase] = useState('')
  const [showPassphrase, setShowPassphrase] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  // 错误以已翻译过的 i18n key 形式存储 — 不放原始 error.code,避免外层重渲染时
  // 失去 t() 上下文。clearOnInputChange 在用户继续输入时被重置。
  const [errorKey, setErrorKey] = useState<string | null>(null)
  const [showKeychainModal, setShowKeychainModal] = useState(false)
  const [verifying, setVerifying] = useState(false)
  const [verifyError, setVerifyError] = useState<string | null>(null)

  // ── Factory reset modal state ─────────────────────────────────────────
  // 兜底路径:用户忘记口令时,通过二次确认对话框 (输入 `RESET`) 触发删
  // keyslot + KEK,然后让 App.tsx 把 UI 切回 SetupPage。设计文档:
  // `unlock_fallback_plan.md`。
  const [showResetModal, setShowResetModal] = useState(false)
  const [resetConfirmInput, setResetConfirmInput] = useState('')
  const [resetting, setResetting] = useState(false)
  const [resetErrorKey, setResetErrorKey] = useState<string | null>(null)

  /**
   * 入口:用户点 Unlock 按钮。
   *
   * 1. 先尝试 silent (keyring) unlock(in-process,无 passphrase)
   * 2. `resumed=true` → 直接成功通知 parent
   * 3. `resumed=false` 或 reject → 弹 passphrase modal 让用户手动输入
   *
   * 区别于原行为(旧版只把 silent 失败 log warn 然后停止):新增 modal 兜底
   * 是修复"keyring 与磁盘 keyslot 漂移"场景下用户被卡死的产品缺口。
   */
  const handleUnlock = async () => {
    setUnlocking(true)
    try {
      const unlocked = await unlockEncryptionSession()
      if (unlocked) {
        // Silent unlock 成功 — session 立即 ready,通知 parent。
        // 不依赖 WS 异步推送, 避免首屏闪烁。
        onUnlockSucceeded?.()
        return
      }
      // Silent unlock 返回 false = "keyring 没东西可恢复"。在 setup 已完成
      // 的前提下这其实是漂移/损坏 — 弹 modal 让用户用口令兜底。
      log.warn('Silent unlock returned false — opening passphrase modal as fallback')
      openPassphraseModal()
    } catch (error) {
      // Silent unlock 抛错 = keyring 与 keyslot 漂移 / FacadeUnavailable / 其他
      // 异常。一律弹 modal:让用户用口令派生 KEK + 刷新 keyring(unlock 内部
      // 的 store_kek refresh),把漂移修好。
      log.warn({ err: error }, 'Silent unlock rejected — opening passphrase modal as fallback')
      openPassphraseModal()
    } finally {
      setUnlocking(false)
    }
  }

  const openPassphraseModal = () => {
    setPassphrase('')
    setShowPassphrase(false)
    setErrorKey(null)
    setShowPassphraseModal(true)
  }

  const closePassphraseModal = () => {
    setShowPassphraseModal(false)
    setPassphrase('')
    setShowPassphrase(false)
    setErrorKey(null)
  }

  /**
   * 用户在 modal 提交明文口令:
   * - 成功 → 关闭 modal + 通知 parent
   * - WRONG_PASSPHRASE → 保留 modal,清空 errorKey 之外的 state,提示重输
   * - 其他错误码 → 保留 modal,展示对应引导文案,**不**自动清空 passphrase
   *   (用户可能想编辑后重提交)
   */
  const handleSubmitPassphrase = async () => {
    const trimmed = passphrase
    if (trimmed.length === 0) {
      // 空口令理论上和 WrongPassphrase 等价,但提前拦截可以省一次 Tauri IPC。
      setErrorKey('unlock.errors.wrongPassphrase')
      return
    }
    setSubmitting(true)
    setErrorKey(null)
    try {
      await unlockSpaceWithPassphrase(trimmed)
      // session 已 ready;同进程 daemon 会被 parent 触发 lifecycle/ready
      // (App.tsx 现有路径)启动 deferred services。
      closePassphraseModal()
      onUnlockSucceeded?.()
    } catch (error) {
      if (isUnlockSpaceError(error)) {
        setErrorKey(unlockErrorI18nKey(error))
      } else {
        // 非 typed 错误 — 例如 IPC bridge 自身的异常。展通用错误。
        log.error({ err: error }, 'Unexpected non-typed unlock error')
        setErrorKey('unlock.errors.internal')
      }
    } finally {
      setSubmitting(false)
    }
  }

  const handlePassphraseKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    // Enter 直接提交,跟 setup screen 一致的输入习惯。
    if (e.key === 'Enter' && !submitting) {
      handleSubmitPassphrase()
    }
  }

  const handlePassphraseChange = (value: string) => {
    setPassphrase(value)
    // 用户开始重新输入 → 清掉旧的错误提示,UI 反馈更顺。
    if (errorKey !== null) {
      setErrorKey(null)
    }
  }

  const handleAutoUnlockChange = async (checked: boolean) => {
    if (checked && isMac) {
      setVerifyError(null)
      setShowKeychainModal(true)
      return
    }
    await updateSecuritySetting({ autoUnlockEnabled: checked })
  }

  const handleKeychainVerify = async () => {
    setVerifying(true)
    setVerifyError(null)
    try {
      const granted = await verifyKeychainAccess()
      if (granted) {
        await updateSecuritySetting({ autoUnlockEnabled: true })
        setShowKeychainModal(false)
      } else {
        setVerifyError(t('unlock.keychainModal.error'))
      }
    } catch {
      setVerifyError(t('unlock.keychainModal.error'))
    } finally {
      setVerifying(false)
    }
  }

  const handleKeychainCancel = () => {
    setShowKeychainModal(false)
    setVerifyError(null)
  }

  // ── Factory reset handlers ────────────────────────────────────────────

  const openResetModal = () => {
    setResetConfirmInput('')
    setResetErrorKey(null)
    setShowResetModal(true)
  }

  const closeResetModal = () => {
    if (resetting) return
    setShowResetModal(false)
    setResetConfirmInput('')
    setResetErrorKey(null)
  }

  const resetConfirmTokenMatches = resetConfirmInput.trim() === FACTORY_RESET_CONFIRMATION_TOKEN

  /**
   * 用户在二次确认 modal 输入 `RESET` 后点 "重置":
   * - 成功 → 关闭 modal + 通知 parent 把 encryption status 缓存重置,让
   *   App.tsx 渲染分支立即切回 SetupPage,避免短暂闪烁。
   * - typed error → 按 code 显示对应文案,保留 modal 让用户决定下一步
   *   (重试 / 重启)。
   */
  const handleResetSubmit = async () => {
    if (!resetConfirmTokenMatches || resetting) return
    setResetting(true)
    setResetErrorKey(null)
    try {
      await resetSpace()
      // **关键**: 仅清 encryption status 不足以让 UI 切回 SetupPage —— SetupPage
      // 的渲染由 `setupRealtimeStore.flow` 控制 (App.tsx:189 `isSetupActive`),
      // 而 reset 只清了 daemon 端的 `setup_status`,前端 store 不会自动感知。
      // 主动 refresh 让 store 从 daemon 拉到 `has_completed=false` → flow 切回
      // `entry`,`isSetupActive` 变 true,SetupPage 才会渲染。
      // refresh 失败不阻塞 reset 成功路径 —— parent 回调仍照常通知,最坏情况
      // 是 UI 落到 "无 SetupPage 也无 UnlockPage" 的灰色态,用户重启即可恢复。
      try {
        await refreshSetupState()
      } catch (refreshErr) {
        log.warn(
          { err: refreshErr },
          'refreshSetupState failed after reset; UI may need restart to recover'
        )
      }
      setShowResetModal(false)
      setResetConfirmInput('')
      // 关闭可能已打开的 passphrase modal —— reset 成功后用户应进入 setup
      // 流程,不应再看到 unlock 相关的覆盖层。
      setShowPassphraseModal(false)
      setPassphrase('')
      setErrorKey(null)
      onResetSucceeded?.()
    } catch (error) {
      if (isFactoryResetError(error)) {
        setResetErrorKey(factoryResetErrorI18nKey(error))
      } else {
        log.error({ err: error }, 'Unexpected non-typed factory reset error')
        setResetErrorKey('unlock.factoryReset.errors.internal')
      }
    } finally {
      setResetting(false)
    }
  }

  return (
    <div className="relative flex min-h-screen w-full flex-col items-center justify-center overflow-hidden p-4">
      <div
        data-uc-decorative-effect="true"
        className="absolute inset-0 bg-gradient-to-b from-transparent via-transparent to-muted/20"
      />

      <div
        data-uc-decorative-effect="true"
        className="absolute -bottom-24 -right-16 size-96 rounded-full bg-primary/5 blur-3xl"
      />

      <div className="relative z-10 flex w-full max-w-sm flex-col items-center gap-y-8 text-center">
        <div className="relative flex size-24 items-center justify-center rounded-3xl bg-muted/30 shadow-inner ring-1 ring-border/50">
          <div className="absolute inset-0 rounded-3xl bg-gradient-to-br from-primary/10 to-transparent opacity-50" />
          <Lock className="size-10 text-primary" />
        </div>

        <div className="space-y-2">
          <h1 className="text-3xl font-bold tracking-tight text-foreground sm:text-4xl">
            {t('unlock.title')}
          </h1>
          <p className="text-muted-foreground">{t('unlock.description')}</p>
        </div>

        <div className="w-full space-y-6">
          <Button
            size="lg"
            className="h-12 w-full rounded-xl text-base font-medium shadow-lg shadow-primary/20 transition-all hover:scale-[1.02] hover:shadow-primary/30"
            onClick={handleUnlock}
            disabled={unlocking}
          >
            {unlocking ? (
              <>
                <Loader2 className="mr-2 size-5 animate-spin" />
                {t('unlock.unlocking')}
              </>
            ) : (
              <>
                <Unlock className="mr-2 size-5" />
                {t('unlock.button')}
              </>
            )}
          </Button>

          <div className="flex items-center justify-between rounded-xl border border-border/40 bg-muted/20 px-4 py-3 backdrop-blur-sm transition-colors hover:bg-muted/30">
            <div className="flex flex-col items-start gap-y-0.5 text-left">
              <Label htmlFor="auto-unlock" className="cursor-pointer text-sm font-medium">
                {t('unlock.autoUnlock.label')}
              </Label>
              <span className="text-xs text-muted-foreground">
                {t('unlock.autoUnlock.description')}
              </span>
            </div>
            <Switch
              id="auto-unlock"
              checked={setting?.security?.autoUnlockEnabled ?? false}
              onCheckedChange={handleAutoUnlockChange}
              disabled={settingsLoading}
            />
          </div>
        </div>

        {isMac && (
          <p className="max-w-xs text-xs text-muted-foreground/60">{t('unlock.macOSNote')}</p>
        )}

        {/* Fallback 入口:用户忘记口令或遇到不可恢复的 keyslot 错误时的最后兜底。
            打开二次确认 modal,要求输入 `RESET` 才能真正触发删除。 */}
        <button
          type="button"
          onClick={openResetModal}
          className="text-xs text-muted-foreground/60 underline-offset-4 transition-colors hover:text-muted-foreground hover:underline"
        >
          {t('unlock.factoryReset.link')}
        </button>
      </div>

      <AlertDialog open={showKeychainModal}>
        <AlertDialogContent onEscapeKeyDown={e => e.preventDefault()}>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('unlock.keychainModal.title')}</AlertDialogTitle>
            <AlertDialogDescription>{t('unlock.keychainModal.description')}</AlertDialogDescription>
          </AlertDialogHeader>

          <ol className="list-decimal space-y-2 pl-5 text-sm text-foreground">
            <li>{t('unlock.keychainModal.step1')}</li>
            <li>{t('unlock.keychainModal.step2')}</li>
            <li>{t('unlock.keychainModal.step3')}</li>
          </ol>

          <p className="text-xs text-muted-foreground">{t('unlock.keychainModal.note')}</p>

          {verifyError && (
            <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
              <p className="text-sm font-medium text-destructive">{verifyError}</p>
            </div>
          )}

          <AlertDialogFooter>
            <Button variant="outline" onClick={handleKeychainCancel} disabled={verifying}>
              {t('unlock.keychainModal.cancel')}
            </Button>
            <Button variant="secondary" onClick={handleKeychainVerify} disabled={verifying}>
              {verifying ? (
                <>
                  <Loader2 className="mr-2 size-4 animate-spin" />
                  {t('unlock.keychainModal.verifying')}
                </>
              ) : (
                t('unlock.keychainModal.confirm')
              )}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={showPassphraseModal}>
        <AlertDialogContent
          onEscapeKeyDown={event => {
            if (submitting) {
              event.preventDefault()
              return
            }
            closePassphraseModal()
          }}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>{t('unlock.passphraseModal.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('unlock.passphraseModal.description')}
            </AlertDialogDescription>
          </AlertDialogHeader>

          <div className="space-y-2">
            <Label htmlFor="unlock-passphrase" className="text-sm">
              {t('unlock.passphraseModal.passphraseLabel')}
            </Label>
            <div className="relative">
              <Input
                id="unlock-passphrase"
                type={showPassphrase ? 'text' : 'password'}
                value={passphrase}
                onChange={e => handlePassphraseChange(e.target.value)}
                onKeyDown={handlePassphraseKeyDown}
                disabled={submitting}
                placeholder={t('unlock.passphraseModal.passphrasePlaceholder')}
                className="pr-10"
                autoFocus
                aria-invalid={errorKey !== null}
              />
              <button
                type="button"
                onClick={() => setShowPassphrase(v => !v)}
                disabled={submitting}
                aria-label={t(
                  showPassphrase ? 'unlock.passphraseModal.hide' : 'unlock.passphraseModal.show'
                )}
                className="absolute right-0 top-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-50"
              >
                {showPassphrase ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
              </button>
            </div>
          </div>

          {errorKey && (
            <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
              <p className="text-sm font-medium text-destructive">{t(errorKey)}</p>
            </div>
          )}

          <p className="text-xs text-muted-foreground">{t('unlock.passphraseModal.hint')}</p>

          {/* 同 macOSNote 下方的链接,在 modal 里也提供一个入口 —— 用户卡在
              口令重试时不必关闭 modal 也能进入 reset 流程。 */}
          <button
            type="button"
            onClick={() => {
              closePassphraseModal()
              openResetModal()
            }}
            disabled={submitting}
            className="self-start text-xs text-muted-foreground/70 underline-offset-4 transition-colors hover:text-muted-foreground hover:underline disabled:opacity-50"
          >
            {t('unlock.factoryReset.link')}
          </button>

          <AlertDialogFooter>
            <Button variant="outline" onClick={closePassphraseModal} disabled={submitting}>
              {t('unlock.passphraseModal.cancel')}
            </Button>
            <Button onClick={handleSubmitPassphrase} disabled={submitting}>
              {submitting ? (
                <>
                  <Loader2 className="mr-2 size-4 animate-spin" />
                  {t('unlock.passphraseModal.submitting')}
                </>
              ) : (
                t('unlock.passphraseModal.submit')
              )}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={showResetModal}>
        <AlertDialogContent
          onEscapeKeyDown={event => {
            if (resetting) {
              event.preventDefault()
              return
            }
            closeResetModal()
          }}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>{t('unlock.factoryReset.modal.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('unlock.factoryReset.modal.warning')}
            </AlertDialogDescription>
          </AlertDialogHeader>

          <p className="text-sm text-muted-foreground">{t('unlock.factoryReset.modal.recovery')}</p>

          <div className="space-y-2">
            <Label htmlFor="factory-reset-confirm" className="text-sm">
              {t('unlock.factoryReset.modal.confirmPrompt')}
            </Label>
            <Input
              id="factory-reset-confirm"
              type="text"
              value={resetConfirmInput}
              onChange={e => setResetConfirmInput(e.target.value)}
              placeholder={t('unlock.factoryReset.modal.confirmPlaceholder')}
              disabled={resetting}
              autoFocus
              autoComplete="off"
              spellCheck={false}
            />
          </div>

          {resetErrorKey && (
            <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
              <p className="text-sm font-medium text-destructive">{t(resetErrorKey)}</p>
            </div>
          )}

          <AlertDialogFooter>
            <Button variant="outline" onClick={closeResetModal} disabled={resetting}>
              {t('unlock.factoryReset.modal.cancel')}
            </Button>
            <Button
              variant="destructive"
              onClick={handleResetSubmit}
              disabled={!resetConfirmTokenMatches || resetting}
            >
              {resetting ? (
                <>
                  <Loader2 className="mr-2 size-4 animate-spin" />
                  {t('unlock.factoryReset.modal.resetting')}
                </>
              ) : (
                t('unlock.factoryReset.modal.confirm')
              )}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
