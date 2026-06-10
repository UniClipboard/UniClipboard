import { useRef, type KeyboardEvent } from 'react'
import { cn } from '@/lib/utils'

interface IpAddressInputProps {
  value: string
  onChange: (value: string) => void
  disabled?: boolean
  placeholder?: string
  className?: string
  onSubmit?: () => void
}

function parseOctets(ip: string): [string, string, string, string] {
  const parts = ip.split('.')
  return [parts[0] ?? '', parts[1] ?? '', parts[2] ?? '', parts[3] ?? '']
}

function joinOctets(octets: [string, string, string, string]): string {
  if (octets.every(o => o === '')) return ''
  return octets.join('.')
}

export function IpAddressInput({
  value,
  onChange,
  disabled,
  className,
  onSubmit,
}: IpAddressInputProps) {
  const refs = [
    useRef<HTMLInputElement>(null),
    useRef<HTMLInputElement>(null),
    useRef<HTMLInputElement>(null),
    useRef<HTMLInputElement>(null),
  ]
  const octets = parseOctets(value)

  const handleChange = (index: number, raw: string) => {
    const digits = raw.replace(/\D/g, '').slice(0, 3)
    const next = [...octets] as [string, string, string, string]
    next[index] = digits
    onChange(joinOctets(next))

    if (digits.length === 3 && index < 3) {
      refs[index + 1].current?.focus()
      refs[index + 1].current?.select()
    }
  }

  const handleKeyDown = (index: number, e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === '.' || (e.key === 'ArrowRight' && index < 3)) {
      e.preventDefault()
      refs[index + 1]?.current?.focus()
      refs[index + 1]?.current?.select()
    } else if (e.key === 'ArrowLeft' && index > 0) {
      e.preventDefault()
      refs[index - 1]?.current?.focus()
      refs[index - 1]?.current?.select()
    } else if (e.key === 'Backspace' && octets[index] === '' && index > 0) {
      e.preventDefault()
      refs[index - 1].current?.focus()
    } else if (e.key === 'Enter' && onSubmit) {
      onSubmit()
    }
  }

  const handlePaste = (e: React.ClipboardEvent) => {
    const text = e.clipboardData.getData('text').trim()
    const match = text.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/)
    if (match) {
      e.preventDefault()
      const pasted: [string, string, string, string] = [match[1], match[2], match[3], match[4]]
      onChange(joinOctets(pasted))
      refs[3].current?.focus()
    }
  }

  const octetClass = cn(
    'w-12 h-9 text-center font-mono text-sm',
    'border-0 border-b border-border/40 bg-transparent',
    'shadow-none rounded-none',
    'focus-visible:border-primary focus-visible:ring-0 focus-visible:outline-none',
    'transition-colors'
  )

  return (
    <div
      className={cn('flex items-center justify-center gap-0.5', className)}
      onPaste={handlePaste}
    >
      {([0, 1, 2, 3] as const).map(i => (
        <div key={i} className="flex items-center">
          <input
            ref={refs[i]}
            type="text"
            inputMode="numeric"
            value={octets[i]}
            onChange={e => handleChange(i, e.target.value)}
            onKeyDown={e => handleKeyDown(i, e)}
            onFocus={e => e.target.select()}
            disabled={disabled}
            className={octetClass}
            maxLength={3}
            placeholder="0"
            aria-label={`IP octet ${i + 1}`}
          />
          {i < 3 && <span className="px-0.5 text-muted-foreground/50">.</span>}
        </div>
      ))}
    </div>
  )
}
