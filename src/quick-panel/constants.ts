import { Code, ExternalLink, File, FileText, Image as ImageIcon } from 'lucide-react'
import React from 'react'
import type { ItemType } from '@/lib/clipboard-utils'

export const PREVIEW_OPEN_DELAY_MS = 500
export const PREVIEW_SWITCH_DELAY_MS = 120

export const isMac = navigator.platform.toUpperCase().includes('MAC')

export const typeIcons: Record<ItemType, React.ElementType> = {
  text: FileText,
  image: ImageIcon,
  link: ExternalLink,
  code: Code,
  file: File,
  unknown: FileText,
}

export const quickCardClassName =
  'flex h-full w-full min-w-0 flex-col overflow-hidden rounded-xl border border-border/50 bg-background/95 shadow-xl backdrop-blur-xl'
