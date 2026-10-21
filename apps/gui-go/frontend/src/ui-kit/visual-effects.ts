import { createContext, use } from 'react'

// Presentation-only context: skins also render the build-time startup screen,
// which must never load host bindings or the live preference store.
export const GuiVisualEffects = createContext({ reduceMotion: true, lowEffects: true })

export const useGuiVisualEffects = () => use(GuiVisualEffects)
