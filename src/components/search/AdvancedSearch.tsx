import { AnimatePresence, LayoutGroup, m } from 'framer-motion'
import { Zap, X, File, Hash } from 'lucide-react'
import React, { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { defaultSearchTagOptions, type SearchTagOption } from '@/lib/search-tags'
import { cn } from '@/lib/utils'

// Shared Search Constants
const TYPE_SUGGESTIONS = ['text', 'image', 'file']
const EXT_SUGGESTIONS = ['txt', 'md', 'jpg', 'png', 'pdf', 'ts', 'js', 'json', 'rs', 'go']

export interface AdvancedSearchProps {
  value: string
  onValueChange: (value: string) => void
  isAdvanced: boolean
  onAdvancedChange: (isAdvanced: boolean) => void
  tokens: string[]
  onTokensChange: (tokens: string[]) => void
  /** Optional control rendered at the leading edge, before the input (e.g. a filter dropdown). */
  leftSlot?: React.ReactNode
  placeholder?: string
  advancedPlaceholder?: string
  className?: string
  inputRef?: React.Ref<HTMLInputElement>
  onKeyDown?: (e: KeyboardEvent) => void
  tagOptions?: SearchTagOption[]
}

function setInputNodeRef(
  ref: React.Ref<HTMLInputElement> | undefined,
  node: HTMLInputElement | null
) {
  if (!ref) {
    return
  }

  if (typeof ref === 'function') {
    ref(node)
    return
  }

  ;(ref as React.MutableRefObject<HTMLInputElement | null>).current = node
}

const AdvancedSearch: React.FC<AdvancedSearchProps> = ({
  value,
  onValueChange,
  isAdvanced,
  onAdvancedChange,
  tokens,
  onTokensChange,
  leftSlot,
  placeholder = 'Search clipboard history…',
  advancedPlaceholder = 'Filter…',
  className,
  inputRef: externalInputRef,
  onKeyDown: externalOnKeyDown,
  tagOptions = defaultSearchTagOptions(),
}) => {
  const inputRef = useRef<HTMLInputElement | null>(
    null
  ) as React.MutableRefObject<HTMLInputElement | null>
  const [suggestionIndex, setSuggestionIndex] = useState(0)
  const [isComposing, setIsComposing] = useState(false)
  const [composingValue, setComposingValue] = useState('')
  const isComposingRef = useRef(false)
  const ignoreNextChangeValueRef = useRef<string | null>(null)
  const suppressEnterUntilRef = useRef(0)

  const assignInputRef = useCallback(
    (node: HTMLInputElement | null) => {
      inputRef.current = node
      setInputNodeRef(externalInputRef, node)
    },
    [externalInputRef]
  )

  // Suggestions Logic
  const suggestions = useMemo(() => {
    if (!isAdvanced || !value.trim()) return []
    const lastWord = value.trim()
    const hasTypeToken = tokens.some(t => t.startsWith('type:'))
    const hasExtToken = tokens.some(t => t.startsWith('ext:'))

    if (lastWord.startsWith('type:')) {
      const q = lastWord.slice(5)
      return TYPE_SUGGESTIONS.flatMap(s => (s.startsWith(q) ? [`type:${s}`] : []))
    }
    if (lastWord.startsWith('#')) {
      const q = lastWord.slice(1).toLowerCase()
      return tagOptions.flatMap(tag => (tag.id.toLowerCase().startsWith(q) ? [`#${tag.id}`] : []))
    }
    if (lastWord.startsWith('ext:')) {
      const q = lastWord.slice(4)
      return EXT_SUGGESTIONS.flatMap(s => (s.startsWith(q) ? [`ext:${s}`] : []))
    }

    const prefixSuggestions = []
    if (!hasTypeToken && 'type:'.startsWith(lastWord)) prefixSuggestions.push('type:')
    if (!hasExtToken && 'ext:'.startsWith(lastWord)) prefixSuggestions.push('ext:')
    if ('#'.startsWith(lastWord)) prefixSuggestions.push('#')
    return prefixSuggestions
  }, [value, isAdvanced, tokens, tagOptions])

  const addToken = useCallback(
    (token: string) => {
      if (!tokens.includes(token)) {
        onTokensChange([...tokens, token])
      }
      onValueChange('')
      setSuggestionIndex(0)
    },
    [tokens, onTokensChange, onValueChange]
  )

  const removeToken = useCallback(
    (index: number) => {
      const newTokens = [...tokens]
      newTokens.splice(index, 1)
      onTokensChange(newTokens)
    },
    [tokens, onTokensChange]
  )

  const applySuggestion = useCallback(
    (val: string) => {
      if (
        (val.includes(':') && val.split(':')[1].length > 0) ||
        (val.startsWith('#') && val.length > 1)
      ) {
        addToken(val)
      } else {
        onValueChange(val)
      }
      setSuggestionIndex(0)
      // Focus input after state updates settle (next microtask) so React has
      // committed any DOM changes triggered by addToken/onValueChange first.
      queueMicrotask(() => {
        inputRef.current?.focus()
      })
    },
    [onValueChange, addToken]
  )

  const commitValue = useCallback(
    (newVal: string) => {
      if (!isAdvanced && (newVal === ':' || newVal === '：')) {
        onAdvancedChange(true)
        onValueChange('')
        return
      }
      if (!isAdvanced && newVal.startsWith('#')) {
        // Any leading `#` is tag syntax — switch to advanced and preserve the
        // full value so a pasted `#code` parses as a tag token instead of a
        // literal search string. (`parseTokens` ignores an empty tag value.)
        onAdvancedChange(true)
        onValueChange(newVal)
        return
      }
      if (isAdvanced && newVal.endsWith(' ')) {
        const trimmed = newVal.trim()
        if (
          (trimmed.includes(':') && trimmed.split(':')[1].length > 0) ||
          (trimmed.startsWith('#') && trimmed.length > 1)
        ) {
          addToken(trimmed)
          return
        }
      }
      onValueChange(newVal)
      setSuggestionIndex(0)
    },
    [addToken, isAdvanced, onAdvancedChange, onValueChange]
  )

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newVal = e.target.value
    if (isComposingRef.current) {
      setComposingValue(newVal)
      setSuggestionIndex(0)
      return
    }
    if (ignoreNextChangeValueRef.current === newVal) {
      ignoreNextChangeValueRef.current = null
      return
    }
    ignoreNextChangeValueRef.current = null
    commitValue(newVal)
  }

  const handleCompositionStart = () => {
    isComposingRef.current = true
    setIsComposing(true)
    setComposingValue(value)
  }

  const handleCompositionEnd = (e: React.CompositionEvent<HTMLInputElement>) => {
    const finalValue = e.currentTarget.value
    isComposingRef.current = false
    ignoreNextChangeValueRef.current = finalValue
    suppressEnterUntilRef.current = Date.now() + 32
    setIsComposing(false)
    setComposingValue('')
    commitValue(finalValue)
  }

  // 这些回调只在 keydown 触发时读一次，不应该让 effect 重新订阅 ——
  // 否则父组件每次 render 都会 detach/re-attach listener，热路径下浪费。
  // 用 useEffectEvent 锁住最新引用，effect 依赖只保留真正影响订阅的 state。
  const onAdvancedChangeEvent = useEffectEvent(onAdvancedChange)
  const removeTokenEvent = useEffectEvent(removeToken)
  const addTokenEvent = useEffectEvent(addToken)
  const applySuggestionEvent = useEffectEvent(applySuggestion)
  const externalOnKeyDownEvent = useEffectEvent((e: KeyboardEvent) => externalOnKeyDown?.(e))

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const nativeEvent = e as KeyboardEvent & { isComposing?: boolean; keyCode?: number }
      const isImeKey =
        nativeEvent.isComposing ||
        isComposingRef.current ||
        nativeEvent.keyCode === 229 ||
        e.key === 'Process'
      const shouldSuppressEnter =
        e.key === 'Enter' && (isImeKey || Date.now() < suppressEnterUntilRef.current)

      if (shouldSuppressEnter) {
        e.preventDefault()
        e.stopPropagation()
        return
      }

      if (isImeKey) {
        return
      }
      if (isAdvanced && value === '' && tokens.length === 0 && e.key === 'Backspace') {
        e.preventDefault()
        onAdvancedChangeEvent(false)
        return
      }
      if (isAdvanced && value === '' && tokens.length > 0 && e.key === 'Backspace') {
        e.preventDefault()
        removeTokenEvent(tokens.length - 1)
        return
      }
      if (suggestions.length > 0) {
        if (e.key === 'ArrowDown') {
          e.preventDefault()
          setSuggestionIndex(prev => (prev + 1) % suggestions.length)
          return
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault()
          setSuggestionIndex(prev => (prev - 1 + suggestions.length) % suggestions.length)
          return
        }
        if (e.key === 'Enter' || e.key === 'Tab') {
          e.preventDefault()
          applySuggestionEvent(suggestions[suggestionIndex])
          return
        }
      } else if (e.key === 'Tab') {
        // Launcher 模型:焦点必须留在搜索框,所以阻止 Tab 移动焦点;但把事件
        // 透传给父级,由它把 Tab / Shift+Tab 解释为切换内容类型筛选。
        e.preventDefault()
      } else if (isAdvanced && e.key === 'Enter' && value.trim()) {
        e.preventDefault()
        addTokenEvent(value.trim())
      }
      externalOnKeyDownEvent(e)
    }
    const el = inputRef.current
    el?.addEventListener('keydown', handleKeyDown)
    return () => el?.removeEventListener('keydown', handleKeyDown)
  }, [isAdvanced, value, tokens, suggestions, suggestionIndex])

  return (
    <div className={cn('flex flex-col relative', className)}>
      <div className="flex items-center gap-2.5 px-4 py-2 min-h-[38px]">
        {leftSlot && <div className="shrink-0 flex items-center">{leftSlot}</div>}
        {/* Search space (tokens + input) fills the remaining width. */}
        <div className="flex-1 flex flex-wrap items-center gap-1.5 min-w-0">
          <LayoutGroup>
            <AnimatePresence mode="popLayout">
              {isAdvanced &&
                tokens.map((token, idx) => (
                  <m.span
                    key={token}
                    layout
                    initial={{ opacity: 0, scale: 0.8 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.8 }}
                    transition={{ duration: 0.15 }}
                    className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-primary/10 text-primary border border-primary/20 text-[11px] font-medium font-mono leading-none"
                  >
                    {token}
                    <button
                      type="button"
                      onClick={() => removeToken(idx)}
                      className="hover:bg-primary/20 rounded-sm"
                    >
                      <X className="size-2.5" />
                    </button>
                  </m.span>
                ))}
            </AnimatePresence>

            <m.div
              layout="position"
              transition={{ duration: 0.15 }}
              className="flex-1 min-w-[80px]"
            >
              <input
                ref={assignInputRef}
                type="text"
                aria-label={placeholder}
                placeholder={
                  isAdvanced ? (tokens.length > 0 ? '' : advancedPlaceholder) : placeholder
                }
                value={isComposing ? composingValue : value}
                onChange={handleInputChange}
                onCompositionStart={handleCompositionStart}
                onCompositionEnd={handleCompositionEnd}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
                autoSave="off"
                className={cn(
                  'w-full bg-transparent text-[14px] text-foreground outline-none font-medium placeholder:font-normal placeholder:text-muted-foreground/40 leading-tight'
                )}
              />
            </m.div>
          </LayoutGroup>
        </div>

        {/* Right Action: clear */}
        <div className="shrink-0 flex items-center">
          {(value || tokens.length > 0 || isAdvanced) && (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => {
                onValueChange('')
                onTokensChange([])
                if (isAdvanced) onAdvancedChange(false)
              }}
              className="p-1 hover:bg-muted rounded-full text-muted-foreground/60 hover:text-foreground transition-colors"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* Suggestions List Overlay */}
      <AnimatePresence>
        {isAdvanced && suggestions.length > 0 && (
          <m.div
            initial={{ opacity: 0, y: -5 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -5 }}
            className="absolute top-full left-4 right-4 z-50 mt-1 rounded-lg border border-border bg-background/95 shadow-2xl backdrop-blur-xl p-1 overflow-hidden"
          >
            {suggestions.map((suggestion, idx) => (
              <button
                type="button"
                key={suggestion}
                onClick={() => applySuggestion(suggestion)}
                className={cn(
                  'flex w-full items-center gap-2.5 px-3 py-1.5 text-[12px] rounded transition-colors text-left',
                  idx === suggestionIndex
                    ? 'bg-primary text-primary-foreground shadow shadow-primary/20'
                    : 'text-foreground hover:bg-muted'
                )}
              >
                {suggestion.startsWith('#') ? (
                  <Hash className="size-3.5 opacity-70" />
                ) : suggestion.includes('type:') ? (
                  <Zap className="size-3.5 opacity-70" />
                ) : (
                  <File className="size-3.5 opacity-70" />
                )}
                <span className="flex-1 font-mono">{suggestion}</span>
                {idx === suggestionIndex && (
                  <span className="text-[9px] opacity-70 font-medium px-1 py-0.5 rounded bg-black/10">
                    TAB
                  </span>
                )}
              </button>
            ))}
          </m.div>
        )}
      </AnimatePresence>
    </div>
  )
}

export default AdvancedSearch
