import { createContext, useContext } from 'react'

/** The SplitView title and subtitle, which the Windows skin draws at the head of the list pane (board WA2). */
export interface SplitViewTitleValue {
  title: string
  subtitle?: string
}

const TitleContext = createContext<SplitViewTitleValue | null>(null)

export const SplitViewTitle = TitleContext.Provider
export const useSplitViewTitle = () => useContext(TitleContext)
