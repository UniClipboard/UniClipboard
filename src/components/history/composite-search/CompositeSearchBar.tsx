import CompositeSearchClearShortcut from './CompositeSearchClearShortcut'
import CompositeSearchInput from './CompositeSearchInput'
import { type CompositeSearchBarProps, useCompositeSearchBar } from './useCompositeSearchBar'

function CompositeSearchBar(props: CompositeSearchBarProps) {
  const state = useCompositeSearchBar(props)

  return (
    <>
      {props.clearShortcutEnabled !== false && (
        <CompositeSearchClearShortcut
          enabled={state.hasContent}
          onClear={() => state.clearAll({ refocus: false })}
        />
      )}
      <CompositeSearchInput
        inputRef={props.inputRef}
        buffer={state.buffer}
        open={state.open}
        panelId={state.panelId}
        chips={state.chips}
        visibleChips={state.visibleChips}
        hiddenChipCount={state.hiddenChipCount}
        options={state.options}
        expanded={state.expanded}
        clampedHighlight={state.clampedHighlight}
        hasContent={state.hasContent}
        totalCount={props.totalCount}
        title={state.t('history.composite.title')}
        placeholder={state.t('history.searchPlaceholder')}
        countLabel={state.t('history.subtitle', { count: props.totalCount })}
        moreFiltersLabel={state.t('history.composite.moreFilters', {
          count: state.hiddenChipCount,
        })}
        clearAllLabel={state.t('history.composite.clearAll')}
        openFiltersLabel={state.t('history.composite.openFilters')}
        showFilterPanelButton={props.showFilterPanelButton === true}
        onInputChange={state.handleInputChange}
        onInputKeyDown={state.handleKeyDown}
        openOnFocus={state.openOnFocus}
        onOpenChange={state.setOpen}
        onOpenFilters={state.openFilters}
        onClearAll={() => state.clearAll()}
        onSeedDimension={state.seedDimension}
        onResetDimension={state.resetDimension}
        onSelectOption={state.selectOption}
        onHighlight={state.setHighlight}
        className={props.className}
      />
    </>
  )
}

export default CompositeSearchBar
