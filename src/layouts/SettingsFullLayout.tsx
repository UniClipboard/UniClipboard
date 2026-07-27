import { Outlet } from 'react-router'

/**
 * Layout for the full-screen settings page.
 * This layout does not include the main sidebar and has a simpler structure.
 *
 * Note: This is rendered within WindowShell, so no need for h-screen wrapper.
 * WindowShell already provides the height constraint via flex-col structure.
 */
const SettingsFullLayout = () => {
  return (
    <div className="w-full h-full flex flex-col">
      <Outlet />
    </div>
  )
}

export default SettingsFullLayout
