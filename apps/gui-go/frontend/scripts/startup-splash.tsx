// Build-time rendering only: the static document uses the same skin as the first React frame.
import {
  createDOMRenderer,
  RendererProvider,
  renderToStyleElements,
} from '@fluentui/react-components'
import { renderToStaticMarkup } from 'react-dom/server'
import {
  upgradeProgressEn,
  upgradeProgressZh,
  upgradeProgressJa,
  upgradeProgressRu,
  upgradeProgressPt,
} from '@/i18n/upgrade-progress'
import { Button, ButtonRow, EntryPage, ProgressBar, Text, UiRoot } from '@/ui-kit/first-frame'

export function renderStartupSplash() {
  const renderer = createDOMRenderer()
  const markup = (['light', 'dark'] as const)
    .map(colorScheme =>
      renderToStaticMarkup(
        <RendererProvider renderer={renderer}>
          <div className="uc-splash" data-splash-theme={colorScheme}>
            <UiRoot colorScheme={colorScheme}>
              <div className="uc-splash-frame">
                <EntryPage
                  appIcon="./src/updater/app-icon.png"
                  status={{ text: '__uc_category__', busy: true }}
                  title="__uc_title__"
                  description="__uc_description__"
                >
                  <div className="uc-splash-wait">
                    <ProgressBar label="Starting up" />
                    <div className="uc-splash-caption">
                      <Text role="caption" tone="secondary">
                        <span data-copy="processing" />
                      </Text>
                      <Text role="caption" tone="secondary">
                        <span data-copy="elapsed" />
                      </Text>
                    </div>
                  </div>
                  <ButtonRow>
                    <Button icon="download" disabled>
                      <span data-copy="export" />
                    </Button>
                  </ButtonRow>
                </EntryPage>
              </div>
            </UiRoot>
          </div>
        </RendererProvider>,
        { identifierPrefix: `splash-${colorScheme}-` }
      )
    )
    .join('')
    .replace(/__uc_(category|title|description)__/g, (_, key) => `<span data-copy="${key}"></span>`)
  return {
    markup,
    exportCopy: {
      'en-US': upgradeProgressEn.export,
      'zh-CN': upgradeProgressZh.export,
      'zh-TW': upgradeProgressZh.export,
      'ja-JP': upgradeProgressJa.export,
      'ru-RU': upgradeProgressRu.export,
      'pt-BR': upgradeProgressPt.export,
    },
    styles: renderToStaticMarkup(<>{renderToStyleElements(renderer)}</>),
  }
}
