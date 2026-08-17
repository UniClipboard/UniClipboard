import { useCallback, useReducer } from 'react'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '../types'

interface SearchFiltersState {
  query: string
  activeFilter: Filter
  tagFilter: string | null
  sourceFilter: string | null
  extensionFilter: string | null
  timeRange: TimeRangePreset
}

type SearchFiltersAction =
  | { type: 'query'; value: string }
  | { type: 'active-filter'; value: Filter }
  | { type: 'tag-filter'; value: string | null }
  | { type: 'source-filter'; value: string | null }
  | { type: 'extension-filter'; value: string | null }
  | { type: 'time-range'; value: TimeRangePreset }
  | { type: 'cycle-active-filter'; filters: readonly Filter[]; reverse: boolean }

const initialSearchFiltersState: SearchFiltersState = {
  query: '',
  activeFilter: Filter.All,
  tagFilter: null,
  sourceFilter: null,
  extensionFilter: null,
  timeRange: 'all_time',
}

function searchFiltersReducer(
  state: SearchFiltersState,
  action: SearchFiltersAction
): SearchFiltersState {
  switch (action.type) {
    case 'query':
      return { ...state, query: action.value }
    case 'active-filter':
      return { ...state, activeFilter: action.value }
    case 'tag-filter':
      return { ...state, tagFilter: action.value }
    case 'source-filter':
      return { ...state, sourceFilter: action.value }
    case 'extension-filter':
      return { ...state, extensionFilter: action.value }
    case 'time-range':
      return { ...state, timeRange: action.value }
    case 'cycle-active-filter': {
      const base = Math.max(0, action.filters.indexOf(state.activeFilter))
      const count = action.filters.length
      const index = action.reverse ? (base - 1 + count) % count : (base + 1) % count
      return { ...state, activeFilter: action.filters[index] }
    }
  }
}

export function useQuickPanelSearchFilters() {
  const [filters, dispatch] = useReducer(searchFiltersReducer, initialSearchFiltersState)

  return {
    filters,
    setQuery: useCallback((value: string) => dispatch({ type: 'query', value }), []),
    setActiveFilter: useCallback((value: Filter) => dispatch({ type: 'active-filter', value }), []),
    setTagFilter: useCallback(
      (value: string | null) => dispatch({ type: 'tag-filter', value }),
      []
    ),
    setSourceFilter: useCallback(
      (value: string | null) => dispatch({ type: 'source-filter', value }),
      []
    ),
    setExtensionFilter: useCallback(
      (value: string | null) => dispatch({ type: 'extension-filter', value }),
      []
    ),
    setTimeRange: useCallback(
      (value: TimeRangePreset) => dispatch({ type: 'time-range', value }),
      []
    ),
    cycleActiveFilter: useCallback(
      (cycle: readonly Filter[], reverse: boolean) =>
        dispatch({ type: 'cycle-active-filter', filters: cycle, reverse }),
      []
    ),
  }
}

interface NavigationState {
  selectedIndex: number
  hoveredIndex: number | null
  isKeyboardNav: boolean
  hasPointerMovedSinceShow: boolean
  previousFilteredCount: number
  preserveSelectionOnNextListChange: boolean
}

type NavigationAction =
  | { type: 'select'; index: number }
  | { type: 'move'; direction: 1 | -1; itemCount: number }
  | { type: 'hover'; index: number | null }
  | { type: 'use-keyboard' }
  | { type: 'pointer-moved' }
  | { type: 'preserve-selection' }
  | { type: 'list-changed'; itemCount: number }

function navigationReducer(state: NavigationState, action: NavigationAction): NavigationState {
  switch (action.type) {
    case 'select':
      return { ...state, selectedIndex: action.index }
    case 'move':
      return {
        ...state,
        selectedIndex:
          action.direction === 1
            ? Math.min(state.selectedIndex + 1, action.itemCount - 1)
            : Math.max(state.selectedIndex - 1, 0),
      }
    case 'hover':
      return { ...state, hoveredIndex: action.index }
    case 'use-keyboard':
      return { ...state, isKeyboardNav: true }
    case 'pointer-moved':
      return { ...state, hasPointerMovedSinceShow: true, isKeyboardNav: false }
    case 'preserve-selection':
      return { ...state, preserveSelectionOnNextListChange: true }
    case 'list-changed':
      return {
        ...state,
        previousFilteredCount: action.itemCount,
        selectedIndex: state.preserveSelectionOnNextListChange ? state.selectedIndex : 0,
        preserveSelectionOnNextListChange: false,
      }
  }
}

export function useQuickPanelNavigation(itemCount: number) {
  const [navigation, dispatch] = useReducer(navigationReducer, itemCount, count => ({
    selectedIndex: 0,
    hoveredIndex: null,
    isKeyboardNav: true,
    hasPointerMovedSinceShow: false,
    previousFilteredCount: count,
    preserveSelectionOnNextListChange: false,
  }))

  if (navigation.previousFilteredCount !== itemCount) {
    dispatch({ type: 'list-changed', itemCount })
  }

  return {
    navigation,
    select: useCallback((index: number) => dispatch({ type: 'select', index }), []),
    move: useCallback(
      (direction: 1 | -1) => dispatch({ type: 'move', direction, itemCount }),
      [itemCount]
    ),
    setHoveredIndex: useCallback((index: number | null) => dispatch({ type: 'hover', index }), []),
    activateKeyboard: useCallback(() => dispatch({ type: 'use-keyboard' }), []),
    notePointerMoved: useCallback(() => dispatch({ type: 'pointer-moved' }), []),
    preserveSelection: useCallback(() => dispatch({ type: 'preserve-selection' }), []),
  }
}
