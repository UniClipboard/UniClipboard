import { defineCloudflareConfig, type OpenNextConfig } from '@opennextjs/cloudflare'

const config: OpenNextConfig = {
  ...defineCloudflareConfig(),
  buildCommand: 'bun run cf:next-build',
}

export default config
