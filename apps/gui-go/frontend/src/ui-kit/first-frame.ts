// The part of `@/ui-kit` the main window's first frame needs (the startup screen, painted by
// src/startup-screen.tsx before the rest of the app loads). It re-exports the same skin components
// as `@/ui-kit`, from a skin entry that imports only their modules, so the first frame does not
// wait for the whole skin. Pages that are not part of the first frame import from `@/ui-kit`.
export {
  Button,
  ButtonRow,
  Disclosure,
  EntryPage,
  InsetList,
  ProgressBar,
  Text,
  UiRoot,
} from '@ui-skin/first-frame'
