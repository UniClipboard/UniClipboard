/** A rejected query has no result count; it is never an empty successful search. */
export type SearchQueryFailure = 'invalid' | 'unavailable' | 'failed'

export const SEARCH_QUERY_FAILURE_COPY = {
  invalid: {
    title: 'history.kit.queryInvalidTitle',
    description: 'history.kit.queryInvalid',
  },
  unavailable: {
    title: 'history.kit.queryFailedTitle',
    description: 'history.kit.queryUnavailable',
  },
  failed: {
    title: 'history.kit.queryFailedTitle',
    description: 'history.kit.queryFailed',
  },
} as const
