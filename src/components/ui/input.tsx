import * as React from 'react'
import { cn } from '@/lib/utils'

// macOS WKWebView 默认对 <input> 开启拼写检查 / 词典纠错 / 首字母大写,
// 而 UniClipboard 大量输入框装的是 ID / username / URL / 配对码等机器
// 字符串,纠错出来的字符通常会被后端校验直接打回。在 wrapper 层统一关
// 掉,业务代码可通过显式传同名 prop 覆盖(例如真的需要自然语言纠错时)。
function Input({
  className,
  type,
  autoCorrect,
  autoCapitalize,
  spellCheck,
  ...props
}: React.ComponentProps<'input'>) {
  return (
    <input
      type={type}
      data-slot="input"
      autoCorrect={autoCorrect ?? 'off'}
      autoCapitalize={autoCapitalize ?? 'off'}
      spellCheck={spellCheck ?? false}
      className={cn(
        'h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2.5 py-1 text-base transition-colors outline-none file:inline-flex file:h-6 file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:cursor-not-allowed disabled:bg-input/50 disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 md:text-sm dark:bg-input/30 dark:disabled:bg-input/80 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40',
        className
      )}
      {...props}
    />
  )
}

export { Input }
