import { createOpenAPI } from 'fumadocs-openapi/server'

export const openapi = createOpenAPI({
  input: ['./openapi/mobile-lan.yaml'],
})
