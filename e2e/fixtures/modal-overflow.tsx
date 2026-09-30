// Browser-only fixture: renders the shared modal primitives already open, so geometry can be measured.
import { createRoot } from 'react-dom/client'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import '@/i18n'
import '@/styles/globals.css'

const params = new URLSearchParams(location.search)
const kind = params.get('kind') ?? 'dialog'
const content = params.get('content') ?? 'short'

const paragraphs = (count: number) =>
  Array.from({ length: count }, (_, index) => (
    <p key={index} className="text-ui-body">
      Paragraph {index + 1}: clipboard history is encrypted before it is persisted.
    </p>
  ))
const body =
  content === 'long'
    ? paragraphs(40)
    : content === 'longword'
      ? [<p key="w">{'A'.repeat(200)}</p>]
      : paragraphs(1)

function Fixture() {
  if (kind === 'alert') {
    return (
      <AlertDialog open>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete history?</AlertDialogTitle>
            <AlertDialogDescription>{body}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    )
  }
  if (kind === 'sheet') {
    return (
      <Sheet open>
        <SheetContent>
          <SheetHeader>
            <SheetTitle>Sheet</SheetTitle>
            <SheetDescription>{body}</SheetDescription>
          </SheetHeader>
        </SheetContent>
      </Sheet>
    )
  }
  if (kind === 'dialog-body') {
    return (
      <Dialog open>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Scrollable body</DialogTitle>
            <DialogDescription>Body scrolls, footer stays.</DialogDescription>
          </DialogHeader>
          <DialogBody>{body}</DialogBody>
          <DialogFooter>Footer</DialogFooter>
        </DialogContent>
      </Dialog>
    )
  }
  return (
    <Dialog open>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Dialog</DialogTitle>
          <DialogDescription>Description</DialogDescription>
        </DialogHeader>
        {body}
        <DialogFooter>Footer</DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

createRoot(document.getElementById('root')!).render(<Fixture />)
