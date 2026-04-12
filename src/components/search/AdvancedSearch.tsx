import { motion, AnimatePresence, LayoutGroup } from 'framer-motion'
import { Search, Zap, X, File } from 'lucide-react'
import React, { useCallback, useMemo, useState, useEffect, useRef } from 'react'
import { cn } from '@/lib/utils'

// Shared Search Constants
export const TYPE_SUGGESTIONS = ['text', 'image', 'link', 'file', 'code']
export const EXT_SUGGESTIONS = ['txt', 'md', 'jpg', 'png', 'pdf', 'ts', 'js', 'json', 'rs', 'go']

export interface AdvancedSearchProps {
  value: string
  onValueChange: (value: string) => void
  isAdvanced: boolean
  onAdvancedChange: (isAdvanced: boolean) => void
  tokens: string[]
  onTokensChange: (tokens: string[]) => void
  icon?: React.ReactNode
  onIconClick?: () => void
  placeholder?: string
  advancedPlaceholder?: string
  className?: string
  inputRef?: React.Ref<HTMLInputElement>
  onKeyDown?: (e: KeyboardEvent) => void
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
  icon,
  onIconClick,
  placeholder = "Search (':' for advanced)...",
  advancedPlaceholder = 'Filter...',
  className,
  inputRef: externalInputRef,
  onKeyDown: externalOnKeyDown,
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
  // State to trigger focus after suggestion application (refs can't be in deps)
  const [focusTrigger, setFocusTrigger] = useState(0)

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
      return TYPE_SUGGESTIONS.filter(s => s.startsWith(q)).map(s => `type:${s}`)
    }
    if (lastWord.startsWith('ext:')) {
      const q = lastWord.slice(4)
      return EXT_SUGGESTIONS.filter(s => s.startsWith(q)).map(s => `ext:${s}`)
    }

    const prefixSuggestions = []
    if (!hasTypeToken && 'type:'.startsWith(lastWord)) prefixSuggestions.push('type:')
    if (!hasExtToken && 'ext:'.startsWith(lastWord)) prefixSuggestions.push('ext:')
    return prefixSuggestions
  }, [value, isAdvanced, tokens])

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
      if (val.includes(':') && val.split(':')[1].length > 0) {
        addToken(val)
      } else {
        onValueChange(val)
      }
      setSuggestionIndex(0)
      // Trigger focus after state updates settle
      setFocusTrigger(t => t + 1)
    },
    [onValueChange, addToken]
  )

  // Focus input whenever focusTrigger changes
  useEffect(() => {
    inputRef.current?.focus()
  }, [focusTrigger])

  const commitValue = useCallback(
    (newVal: string) => {
      if (!isAdvanced && (newVal === ':' || newVal === '：')) {
        onAdvancedChange(true)
        onValueChange('')
        return
      }
      if (isAdvanced && newVal.endsWith(' ')) {
        const trimmed = newVal.trim()
        if (trimmed.includes(':') && trimmed.split(':')[1].length > 0) {
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
        onAdvancedChange(false)
        return
      }
      if (isAdvanced && value === '' && tokens.length > 0 && e.key === 'Backspace') {
        e.preventDefault()
        removeToken(tokens.length - 1)
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
          applySuggestion(suggestions[suggestionIndex])
          return
        }
      } else if (isAdvanced && e.key === 'Enter' && value.trim()) {
        e.preventDefault()
        addToken(value.trim())
      }
      externalOnKeyDown?.(e)
    }
    const el = inputRef.current
    el?.addEventListener('keydown', handleKeyDown)
    return () => el?.removeEventListener('keydown', handleKeyDown)
  }, [
    isAdvanced,
    value,
    tokens,
    suggestions,
    suggestionIndex,
    applySuggestion,
    onAdvancedChange,
    removeToken,
    addToken,
    externalOnKeyDown,
    inputRef,
  ])

  return (
    <div className={cn('flex flex-col relative', className)}>
      <div className="flex items-center gap-3 px-4 pt-2.5 pb-1.5 min-h-[44px]">
        {/* 
            LEFT ANCHOR: Fixed width container ensures the input area 
            NEVER shifts when switching modes.
        */}
        <div className="shrink-0 w-8 h-8 -ml-1 flex items-center justify-center">
          <button
            onClick={onIconClick}
            className="w-full h-full flex items-center justify-center hover:bg-muted/50 rounded transition-colors outline-none"
          >
            <AnimatePresence mode="wait">
              {isAdvanced ? (
                <motion.div
                  key="zap"
                  initial={{ opacity: 0, scale: 0.5, rotate: -20 }}
                  animate={{ opacity: 1, scale: 1, rotate: 0 }}
                  exit={{ opacity: 0, scale: 0.5, rotate: 20 }}
                  transition={{ duration: 0.12 }}
                >
                  <Zap className="h-4 w-4 text-primary fill-primary/20" />
                </motion.div>
              ) : (
                <motion.div
                  key="normal"
                  initial={{ opacity: 0, scale: 0.5 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.5 }}
                  transition={{ duration: 0.12 }}
                >
                  {icon || <Search className="h-4 w-4 text-muted-foreground/60" />}
                </motion.div>
              )}
            </AnimatePresence>
          </button>
        </div>

        {/* Unified Search Space - START POINT IS NOW FIXED */}
        <div className="flex-1 flex flex-wrap items-center gap-1.5 min-w-0">
          <LayoutGroup>
            <AnimatePresence mode="popLayout">
              {isAdvanced &&
                tokens.map((token, idx) => (
                  <motion.span
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
                      onClick={() => removeToken(idx)}
                      className="hover:bg-primary/20 rounded-sm"
                    >
                      <X className="h-2.5 w-2.5" />
                    </button>
                  </motion.span>
                ))}
            </AnimatePresence>

            <motion.div
              layout="position"
              transition={{ duration: 0.15 }}
              className="flex-1 min-w-[80px]"
            >
              <input
                ref={assignInputRef}
                type="text"
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
            </motion.div>
          </LayoutGroup>
        </div>

        {/* Right Action (Clear) */}
        <div className="shrink-0 flex items-center">
          {(value || tokens.length > 0 || isAdvanced) && (
            <button
              onClick={() => {
                onValueChange('')
                onTokensChange([])
                if (isAdvanced) onAdvancedChange(false)
              }}
              className="p-1.5 hover:bg-muted rounded-full text-muted-foreground/60 hover:text-foreground transition-colors"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* Suggestions List Overlay */}
      <AnimatePresence>
        {isAdvanced && suggestions.length > 0 && (
          <motion.div
            initial={{ opacity: 0, y: -5 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -5 }}
            className="absolute top-full left-4 right-4 z-50 mt-1 rounded-lg border border-border bg-background/95 shadow-2xl backdrop-blur-xl p-1 overflow-hidden"
          >
            {suggestions.map((suggestion, idx) => (
              <button
                key={suggestion}
                onClick={() => applySuggestion(suggestion)}
                className={cn(
                  'flex w-full items-center gap-2.5 px-3 py-1.5 text-[12px] rounded transition-colors text-left',
                  idx === suggestionIndex
                    ? 'bg-primary text-primary-foreground shadow shadow-primary/20'
                    : 'text-foreground hover:bg-muted'
                )}
              >
                {suggestion.includes('type:') ? (
                  <Zap className="h-3.5 w-3.5 opacity-70" />
                ) : (
                  <File className="h-3.5 w-3.5 opacity-70" />
                )}
                <span className="flex-1 font-mono">{suggestion}</span>
                {idx === suggestionIndex && (
                  <span className="text-[9px] opacity-70 font-medium px-1 py-0.5 rounded bg-black/10">
                    TAB
                  </span>
                )}
              </button>
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

export default AdvancedSearch
