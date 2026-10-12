import { Popover } from '@base-ui/react/popover'
import { useId, useRef } from 'react'
import type { ComboFieldProps } from '@/ui-kit/contract'
import { Icon } from './icons'
import { isImeKey } from './ime'
import { usePortalContainer } from './portal'
import { swatchColor } from './swatch'

/**
 * Pill-shaped tag field: a focus-ringed entry with a popover menu under it. A
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
    <div className="adw-combo">
      <label ref={anchor} className="adw-combo-field">
        {prefix && <span className="adw-combo-prefix">{prefix}</span>}
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
            className="adw-positioner"
            anchor={anchor}
            sideOffset={6}
            align="start"
            collisionPadding={12}
          >
            <Popover.Popup
              className="adw-menu adw-combo-menu"
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
                    className="adw-combo-option"
                    data-create={suggestion.create ? '' : undefined}
                    data-highlighted={index === highlighted ? '' : undefined}
                    onMouseEnter={() => onHighlightedChange(index)}
                  >
                    <div
                      id={optionId(index)}
                      role="option"
                      aria-selected={index === highlighted}
                      className="adw-suggestion"
                      data-create={suggestion.create ? '' : undefined}
                      onClick={() => onSelectSuggestion(index)}
                    >
                      {suggestion.create ? (
                        <span className="adw-suggestion-create">
                          <Icon name="plus" size={14} />
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
                            className="adw-combo-dot"
                            style={{ background: swatchColor(suggestion.swatch ?? 'gray') }}
                          />
                          <span className="adw-suggestion-label">{suggestion.label}</span>
                        </>
                      )}
                      {suggestion.create && (
                        <span className="adw-combo-return" aria-hidden="true">
                          ↩
                        </span>
                      )}
                      {suggestion.detail && (
                        <span className="adw-suggestion-detail">{suggestion.detail}</span>
                      )}
                    </div>
                    {suggestion.create && createColor && (
                      <div
                        className="adw-combo-colors"
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
                            className="adw-combo-swatch"
                            style={{ background: swatchColor(color) }}
                            tabIndex={-1}
                            onClick={() => createColor.onValueChange(color)}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                ))}
                {footer && <div className="adw-suggestions-footer">{footer}</div>}
              </div>
            </Popover.Popup>
          </Popover.Positioner>
        </Popover.Portal>
      </Popover.Root>
    </div>
  )
}
