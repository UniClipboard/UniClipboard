import { Popover } from '@base-ui/react/popover'
import { useId, useRef } from 'react'
import type { ComboFieldProps } from '@/ui-kit/contract'
import { Icon } from './icons'
import { isImeKey } from './ime'
import { usePortalContainer } from './portal'
import { swatchColor } from './swatch'

/**
 * The capsule tag field of design MC1: a focus-ringed capsule with a glass menu under it. A
 * `create` suggestion carries the colour choice of the new tag.
 */
export function ComboField({
  label,
  prefix,
  placeholder,
  value,
  onValueChange,
  suggestions,
  highlighted,
  onHighlightedChange,
  onSelectSuggestion,
  onKeyDown,
  onDismiss,
  createColor,
  maxLength,
  footer,
  inputRef,
}: ComboFieldProps) {
  const listId = useId()
  const anchor = useRef<HTMLLabelElement>(null)
  const container = usePortalContainer()
  const name = prefix ? `${prefix}${value.trim()}` : ''
  const optionId = (index: number) => `${listId}-option-${index}`
  return (
    <div className="mac-combo">
      <label ref={anchor} className="mac-combo-field">
        {prefix && <span className="mac-combo-prefix">{prefix}</span>}
        <input
          ref={inputRef}
          type="text"
          role="combobox"
          aria-label={label}
          aria-expanded={suggestions.length > 0}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={highlighted >= 0 ? optionId(highlighted) : undefined}
          // The field opens on demand (Add Tag) and takes focus at once.
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus
          placeholder={placeholder}
          maxLength={maxLength}
          value={value}
          onChange={event => onValueChange(event.target.value)}
          onKeyDown={event => {
            if (!isImeKey(event)) onKeyDown?.(event)
          }}
          onBlur={onDismiss}
        />
      </label>
      <Popover.Root
        open={suggestions.length > 0}
        onOpenChange={(open, details) => {
          // The input is the positioning anchor, not a popover trigger. Clicking it keeps editing.
          if (!open && !anchor.current?.contains(details.event.target as Node)) onDismiss()
        }}
      >
        <Popover.Portal container={container}>
          <Popover.Positioner
            className="mac-positioner"
            anchor={anchor}
            sideOffset={6}
            align="start"
            collisionPadding={12}
          >
            <Popover.Popup
              className="mac-menu mac-combo-menu"
              initialFocus={false}
              finalFocus={false}
              // Pointer selection must keep the combobox input focused.
              onMouseDown={event => event.preventDefault()}
            >
              <div id={listId} role="listbox" aria-label={label}>
                {suggestions.map((suggestion, index) => (
                  <div
                    key={suggestion.id}
                    role="presentation"
                    className="mac-combo-option"
                    data-create={suggestion.create ? '' : undefined}
                    data-highlighted={index === highlighted ? '' : undefined}
                    onMouseEnter={() => onHighlightedChange(index)}
                  >
                    <div
                      id={optionId(index)}
                      role="option"
                      aria-selected={index === highlighted}
                      className="mac-suggestion"
                      data-create={suggestion.create ? '' : undefined}
                      onClick={() => onSelectSuggestion(index)}
                    >
                      {suggestion.create ? (
                        <span className="mac-suggestion-create">
                          <Icon name="plus" size={12} />
                          {name && suggestion.label.endsWith(name) ? (
                            <span>
                              {suggestion.label.slice(0, -name.length)}
                              <b>{name}</b>
                            </span>
                          ) : (
                            suggestion.label
                          )}
                        </span>
                      ) : (
                        <>
                          <span
                            className="mac-combo-dot"
                            style={{ background: swatchColor(suggestion.swatch ?? 'gray') }}
                          />
                          <span className="mac-suggestion-label">{suggestion.label}</span>
                        </>
                      )}
                      {suggestion.create && (
                        <span className="mac-combo-return" aria-hidden="true">
                          ↩
                        </span>
                      )}
                      {suggestion.detail && (
                        <span className="mac-suggestion-detail">{suggestion.detail}</span>
                      )}
                    </div>
                    {suggestion.create && createColor && (
                      <div
                        className="mac-combo-colors"
                        role="radiogroup"
                        aria-label={createColor.label}
                      >
                        <span>{createColor.label}</span>
                        {createColor.colors.map(color => (
                          <button
                            key={color}
                            type="button"
                            role="radio"
                            aria-checked={color === createColor.value}
                            aria-label={color}
                            className="mac-combo-swatch"
                            style={{ background: swatchColor(color) }}
                            tabIndex={-1}
                            onClick={() => createColor.onValueChange(color)}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                ))}
                {footer && <div className="mac-suggestions-footer">{footer}</div>}
              </div>
            </Popover.Popup>
          </Popover.Positioner>
        </Popover.Portal>
      </Popover.Root>
    </div>
  )
}
