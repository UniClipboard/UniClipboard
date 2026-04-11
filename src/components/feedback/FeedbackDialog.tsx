import { Loader2 } from 'lucide-react'
import * as React from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { Sentry } from '@/observability/sentry'

const log = createLogger('feedback-dialog')

const FEEDBACK_EMAIL_STORAGE_KEY = 'uniclipboard.feedback.email'

interface FeedbackDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function FeedbackDialog({ open, onOpenChange }: FeedbackDialogProps) {
  const { t } = useTranslation()
  const [content, setContent] = React.useState('')
  const [email, setEmail] = React.useState('')
  const [savedEmail, setSavedEmail] = React.useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = React.useState(false)

  React.useEffect(() => {
    try {
      const storedEmail = localStorage.getItem(FEEDBACK_EMAIL_STORAGE_KEY)
      if (storedEmail) {
        setSavedEmail(storedEmail)
      }
    } catch {
      setSavedEmail(null)
    }
  }, [])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!content.trim()) return

    setIsSubmitting(true)
    try {
      const eventId = Sentry.captureMessage('User Feedback')
      Sentry.captureFeedback({
        message: content,
        name: 'User',
        email: email || undefined,
        associatedEventId: eventId,
      })

      if (email.trim()) {
        try {
          const nextEmail = email.trim()
          localStorage.setItem(FEEDBACK_EMAIL_STORAGE_KEY, nextEmail)
          setSavedEmail(nextEmail)
        } catch {
          setSavedEmail(null)
        }
      }

      toast.success(t('feedback.modal.success'))
      onOpenChange(false)
      setContent('')
      setEmail('')
    } catch (error) {
      log.error({ err: error }, 'Failed to send feedback')
      toast.error(t('feedback.modal.error'))
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>{t('feedback.modal.title')}</DialogTitle>
          <DialogDescription>{t('feedback.modal.description')}</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4 py-4">
          <div className="grid gap-2">
            <label
              htmlFor="feedback-content"
              className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
            >
              {t('feedback.modal.label.content')}
            </label>
            <textarea
              id="feedback-content"
              className={cn(
                'flex min-h-[100px] w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50'
              )}
              placeholder={t('feedback.modal.placeholder.content')}
              value={content}
              onChange={e => setContent(e.target.value)}
              required
            />
          </div>
          <div className="grid gap-2">
            <label
              htmlFor="feedback-email"
              className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
            >
              {t('feedback.modal.label.email')}
            </label>
            <Input
              id="feedback-email"
              type="email"
              placeholder={t('feedback.modal.placeholder.email')}
              value={email}
              onChange={e => setEmail(e.target.value)}
            />
            {savedEmail && (
              <div className="flex justify-end">
                <button
                  type="button"
                  className="text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
                  onClick={() => setEmail(savedEmail)}
                >
                  {t('feedback.modal.useSavedEmail')}
                </button>
              </div>
            )}
          </div>
          <DialogFooter>
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {t('feedback.modal.submit')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
