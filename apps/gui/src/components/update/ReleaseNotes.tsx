import { openUrl } from '@tauri-apps/plugin-opener'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import Markdown from 'react-markdown'
import { createLogger } from '@/lib/logger'

interface ReleaseNotesProps {
  content: string
  fallback: string
}

const log = createLogger('release-notes')

const ZH_SEPARATOR = '<!-- zh -->'

// Links must open in the system browser; default <a> navigation would
// replace the webview content with the external page.
const markdownComponents = {
  a: ({ href, children }: { href?: string; children?: ReactNode }) => (
    <a
      href={href}
      onClick={event => {
        event.preventDefault()
        if (href) {
          openUrl(href).catch(err => log.error({ err, href }, 'Failed to open link'))
        }
      }}
    >
      {children}
    </a>
  ),
}

export function ReleaseNotes({ content, fallback }: ReleaseNotesProps) {
  const { i18n } = useTranslation()
  const body = content?.trim()
  if (!body) return <span className="text-muted-foreground">{fallback}</span>

  let displayContent = body
  if (body.includes(ZH_SEPARATOR)) {
    const [enPart, zhPart] = body.split(ZH_SEPARATOR, 2)
    const zhContent = zhPart?.trim()
    if (i18n.language.startsWith('zh') && zhContent) {
      displayContent = zhContent
    } else {
      displayContent = enPart.trim()
    }
  }

  return (
    <div
      className="text-ui-body break-words [&_:is(h1,h2,h3,h4,h5,h6)]:text-ui-section
                    [&_:is(h1,h2,h3,h4,h5,h6)]:mt-3 [&_:is(h1,h2,h3,h4,h5,h6)]:mb-1
                    [&_ul]:my-1 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:my-1 [&_ol]:list-decimal [&_ol]:pl-5
                    [&_li]:my-0 [&_p]:my-1 [&_a]:underline [&_pre]:overflow-x-auto [&_code]:font-mono"
    >
      <Markdown components={markdownComponents}>{displayContent}</Markdown>
    </div>
  )
}
