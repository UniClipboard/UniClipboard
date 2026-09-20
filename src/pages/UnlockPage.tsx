import { Eye, EyeOff, Loader2, Unlock } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isFactoryResetError, resetSpace, type FactoryResetError } from '@/api/security'
import { AppStateShell } from '@/components/app/AppStateShell'
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
import { contentUnlockErrorKey } from '@/lib/content-unlock-error'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { ensureSetupRealtimeSync, refreshSetupState } from '@/store/setupRealtimeStore'

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

export default function UnlockPage({ onUnlockSucceeded, onResetSucceeded }: UnlockPageProps) {
  const { t } = useTranslation()
  // Manual content access always requires an independently verified passphrase.
  const [showPassphraseModal, setShowPassphraseModal] = useState(false)
  const [passphrase, setPassphrase] = useState('')
  const [showPassphrase, setShowPassphrase] = useState(false)
  const [unlocking, setUnlocking] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  // 错误以已翻译过的 i18n key 形式存储 — 不放原始 error.code,避免外层重渲染时
  // 失去 t() 上下文。clearOnInputChange 在用户继续输入时被重置。
  const [errorKey, setErrorKey] = useState<string | null>(null)

  // ── Factory reset modal state ─────────────────────────────────────────
  // 兜底路径:用户忘记口令时,通过二次确认对话框 (输入 `RESET`) 触发删
  // keyslot + KEK,然后让 App.tsx 把 UI 切回 SetupPage。设计文档:
  // `unlock_fallback_plan.md`。
  const [showResetModal, setShowResetModal] = useState(false)
  const [resetConfirmInput, setResetConfirmInput] = useState('')
  const [resetting, setResetting] = useState(false)
  const [resetErrorKey, setResetErrorKey] = useState<string | null>(null)

  const finishUnlock = async () => {
    try {
      await ensureSetupRealtimeSync()
      await refreshSetupState()
    } catch (error) {
      log.warn({ err: error }, 'Setup state refresh failed after unlock')
    }
    onUnlockSucceeded?.()
  }

  const openPassphraseModal = () => {
    setPassphrase('')
    setShowPassphrase(false)
    setErrorKey(null)
    setShowPassphraseModal(true)
  }

  const handleUnlock = async () => {
    if (unlocking) return
    setUnlocking(true)
    try {
      if (await commands.unlockContentFromKeyring()) {
        await finishUnlock()
        return
      }
    } catch (error) {
      log.warn({ err: error }, 'Keyring unlock failed; requesting the passphrase')
    } finally {
      setUnlocking(false)
    }
    openPassphraseModal()
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
      await commands.unlockContent({ passphrase: trimmed })
      // session 已 ready;同进程 daemon 会被 parent 触发 lifecycle/ready
      // (App.tsx 现有路径)启动 deferred services。
      closePassphraseModal()
      await finishUnlock()
    } catch (error) {
      setErrorKey(contentUnlockErrorKey(error))
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
    <>
      <AppStateShell
        title={t('unlock.title')}
        description={t('unlock.description')}
        width="compact"
      >
        <div className="mt-7">
          <Button
            data-testid="unlock-content"
            className="w-full"
            onClick={handleUnlock}
            disabled={unlocking}
          >
            {unlocking ? (
              <Loader2 className="mr-2 size-5 animate-spin" />
            ) : (
              <Unlock className="mr-2 size-5" />
            )}
            {unlocking ? t('unlock.unlocking') : t('unlock.button')}
          </Button>

          {/* Fallback 入口:用户忘记口令或遇到不可恢复的 keyslot 错误时的最后兜底。
              打开二次确认 modal,要求输入 `RESET` 才能真正触发删除。 */}
          <button
            type="button"
            onClick={openResetModal}
            className="mt-8 text-ui-caption text-muted-foreground/70 underline-offset-4 transition-colors hover:text-muted-foreground hover:underline"
          >
            {t('unlock.factoryReset.link')}
          </button>
        </div>
      </AppStateShell>

      <AlertDialog
        open={showPassphraseModal}
        onOpenChange={(open, eventDetails) => {
          if (open) return
          if (submitting) {
            eventDetails.cancel()
            return
          }
          closePassphraseModal()
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('unlock.passphraseModal.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('unlock.passphraseModal.description')}
            </AlertDialogDescription>
          </AlertDialogHeader>

          <div className="space-y-2">
            <Label htmlFor="unlock-passphrase">{t('unlock.passphraseModal.passphraseLabel')}</Label>
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
              <p className="text-ui-body font-medium text-destructive">{t(errorKey)}</p>
            </div>
          )}

          <p className="text-ui-caption text-muted-foreground">
            {t('unlock.passphraseModal.hint')}
          </p>

          {/* 同 macOSNote 下方的链接,在 modal 里也提供一个入口 —— 用户卡在
              口令重试时不必关闭 modal 也能进入 reset 流程。 */}
          <button
            type="button"
            onClick={() => {
              closePassphraseModal()
              openResetModal()
            }}
            disabled={submitting}
            className="self-start text-ui-body text-muted-foreground/70 underline-offset-4 transition-colors hover:text-muted-foreground hover:underline disabled:opacity-50"
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

      <AlertDialog
        open={showResetModal}
        onOpenChange={(open, eventDetails) => {
          if (open) return
          if (resetting) {
            eventDetails.cancel()
            return
          }
          closeResetModal()
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('unlock.factoryReset.modal.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('unlock.factoryReset.modal.warning')}
            </AlertDialogDescription>
          </AlertDialogHeader>

          <div className="space-y-2">
            <Label htmlFor="factory-reset-confirm">
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
              <p className="text-ui-body font-medium text-destructive">{t(resetErrorKey)}</p>
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
    </>
  )
}
