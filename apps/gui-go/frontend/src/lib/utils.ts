import { type ClassValue, clsx } from 'clsx'
import { extendTailwindMerge } from 'tailwind-merge'

const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      'font-size': [
        'text-ui-caption',
        'text-ui-caption-relaxed',
        'text-ui-body',
        'text-ui-body-relaxed',
        'text-ui-section',
        'text-ui-title',
      ],
    },
  },
})

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export const formatPeerIdForDisplay = (peerId?: string | null, suffixLength = 8) => {
  if (!peerId) return ''
  if (peerId.length <= suffixLength) return peerId
  return `${peerId.slice(-suffixLength)}`
}
