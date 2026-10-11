import type { TagColor } from '@/ui-kit/contract'

/** Tag palette on macOS system colors (design T1, "project tag colors"). */
export const swatchColor: Record<TagColor, string> = {
  deploy: 'var(--orange)',
  docker: 'var(--blue)',
  git: 'var(--gray)',
  design: 'var(--purple)',
  finance: 'var(--green)',
  work: 'var(--blue)',
  neutral: 'var(--gray)',
}
