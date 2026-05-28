import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export const formatPeerIdForDisplay = (peerId?: string | null, suffixLength = 8) => {
  if (!peerId) return ''
  if (peerId.length <= suffixLength) return peerId
  return `${peerId.slice(-suffixLength)}`
}
