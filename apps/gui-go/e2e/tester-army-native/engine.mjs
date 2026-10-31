import { web } from '@e2e-dev/web'

export const nativeEngine = web({
  connect: {
    cdpEndpoint: () => process.env.UC_NATIVE_ENDPOINT,
    reconnectEndpoint: () => process.env.UC_NATIVE_ENDPOINT,
  },
})
