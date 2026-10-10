import type { ReactNode } from 'react'
import type {
  AnimatedToast,
  ToastInput,
  ToastStatus,
} from '@/components/motion/animated-toast-types'

const DEFAULT_DURATION = 4200
const LIMIT = 4
let toasts: AnimatedToast[] = []
const listeners = new Set<() => void>()
let nextId = 0

type ToastOptions = Omit<ToastInput, 'title' | 'status' | 'id'> & { id?: string | number }

function publish(next: AnimatedToast[]) {
  toasts = next
  listeners.forEach(listener => listener())
}

function show(status: ToastStatus, title: ReactNode, options: ToastOptions = {}) {
  const id = String(options.id ?? `toast-${++nextId}`)
  const entry: AnimatedToast = {
    duration: status === 'loading' ? 0 : DEFAULT_DURATION,
    dismissible: true,
    ...options,
    id,
    title,
    status,
    createdAt: Date.now(),
  }
  const exists = toasts.some(item => item.id === id)
  publish(
    exists ? toasts.map(item => (item.id === id ? entry : item)) : [...toasts, entry].slice(-LIMIT)
  )
  return id
}

export const toast = {
  message: (title: ReactNode, options?: ToastOptions) => show('neutral', title, options),
  success: (title: ReactNode, options?: ToastOptions) => show('success', title, options),
  error: (title: ReactNode, options?: ToastOptions) => show('error', title, options),
  info: (title: ReactNode, options?: ToastOptions) => show('info', title, options),
  loading: (title: ReactNode, options?: ToastOptions) => show('loading', title, options),
  dismiss: (id?: string | number) =>
    publish(id === undefined ? [] : toasts.filter(item => item.id !== String(id))),
}

export const toastStore = {
  getSnapshot: () => toasts,
  subscribe(listener: () => void) {
    listeners.add(listener)
    return () => {
      listeners.delete(listener)
    }
  },
}
