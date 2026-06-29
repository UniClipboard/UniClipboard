import CompositeSearchInput from './CompositeSearchInput'
import { type CompositeSearchBarProps, useCompositeSearchBar } from './useCompositeSearchBar'

function CompositeSearchBar(props: CompositeSearchBarProps) {
  const state = useCompositeSearchBar(props)

  return (
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
      onInputChange={state.handleInputChange}
      onInputKeyDown={state.handleKeyDown}
      onOpenChange={state.setOpen}
      onClearAll={() => state.clearAll()}
      onSeedDimension={state.seedDimension}
      onResetDimension={state.resetDimension}
      onSelectOption={state.selectOption}
      onHighlight={state.setHighlight}
    />
  )
}

export default CompositeSearchBar
