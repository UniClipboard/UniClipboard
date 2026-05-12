import { Eye, EyeOff, Loader2, Lock, Unlock } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isUnlockSpaceError,
  unlockEncryptionSession,
  unlockSpaceWithPassphrase,
  verifyKeychainAccess,
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

const log = createLogger('unlock-page')

interface UnlockPageProps {
  onUnlockSucceeded?: () => void
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

export default function UnlockPage({ onUnlockSucceeded }: UnlockPageProps) {
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

  return (
    <div className="relative flex min-h-screen w-full flex-col items-center justify-center overflow-hidden p-4">
      <div className="absolute inset-0 bg-gradient-to-b from-transparent via-transparent to-muted/20" />

      <div className="absolute -bottom-24 -right-16 h-96 w-96 rounded-full bg-primary/5 blur-3xl" />

      <div className="relative z-10 flex w-full max-w-sm flex-col items-center space-y-8 text-center">
        <div className="relative flex h-24 w-24 items-center justify-center rounded-3xl bg-muted/30 shadow-inner ring-1 ring-border/50">
          <div className="absolute inset-0 rounded-3xl bg-gradient-to-br from-primary/10 to-transparent opacity-50" />
          <Lock className="h-10 w-10 text-primary" />
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
                <Loader2 className="mr-2 h-5 w-5 animate-spin" />
                {t('unlock.unlocking')}
              </>
            ) : (
              <>
                <Unlock className="mr-2 h-5 w-5" />
                {t('unlock.button')}
              </>
            )}
          </Button>

          <div className="flex items-center justify-between rounded-xl border border-border/40 bg-muted/20 px-4 py-3 backdrop-blur-sm transition-colors hover:bg-muted/30">
            <div className="flex flex-col items-start space-y-0.5 text-left">
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
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
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
                {showPassphrase ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            </div>
          </div>

          {errorKey && (
            <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
              <p className="text-sm font-medium text-destructive">{t(errorKey)}</p>
            </div>
          )}

          <p className="text-xs text-muted-foreground">{t('unlock.passphraseModal.hint')}</p>

          <AlertDialogFooter>
            <Button variant="outline" onClick={closePassphraseModal} disabled={submitting}>
              {t('unlock.passphraseModal.cancel')}
            </Button>
            <Button onClick={handleSubmitPassphrase} disabled={submitting}>
              {submitting ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t('unlock.passphraseModal.submitting')}
                </>
              ) : (
                t('unlock.passphraseModal.submit')
              )}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
