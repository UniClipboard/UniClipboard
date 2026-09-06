/*!
MIT License

Copyright (c) 2026 Saurabh Chauhan

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/
// Animated Toast Stack, adapted from https://beui.dev/r/animated-toast-stack/raw
import type { ReactNode } from 'react'
export type ToastStatus = 'neutral' | 'info' | 'loading' | 'success' | 'error'
export type ToastPosition =
  | 'top-left'
  | 'top-center'
  | 'top-right'
  | 'bottom-left'
  | 'bottom-center'
  | 'bottom-right'

export type AnimatedToastAction = {
  label: ReactNode
  onClick: (toast: AnimatedToast) => void
}

export type AnimatedToast = {
  id: string
  title: ReactNode
  description?: ReactNode
  status?: ToastStatus
  icon?: ReactNode
  action?: AnimatedToastAction
  duration?: number
  dismissible?: boolean
  createdAt?: number
}

export type ToastInput = Omit<AnimatedToast, 'id' | 'createdAt'> & {
  id?: string
}

export type ToastClassNames = {
  root?: string
  item?: string
  surface?: string
  iconWrap?: string
  content?: string
  title?: string
  description?: string
  action?: string
  close?: string
  progress?: string
}

export interface AnimatedToastStackProps {
  toasts: AnimatedToast[]
  onDismiss?: (id: string) => void
  position?: ToastPosition
  placement?: 'static' | 'fixed' | 'absolute'
  fixed?: boolean
  portal?: boolean
  portalRoot?: Element | null
  maxVisible?: number
  className?: string
  classNames?: ToastClassNames
  icons?: Partial<Record<ToastStatus, ReactNode>>
  renderToast?: (toast: AnimatedToast) => ReactNode
  label?: string
  dismissLabel?: string
}
