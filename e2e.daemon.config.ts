import config from './e2e.config'

if (process.platform !== 'darwin')
  throw new Error('This daemon acceptance requires macOS isolation')

export default {
  ...config,
  tests: ['apps/gui-go/frontend/e2e/tester-army/daemon-history.e2e.mjs'],
  output: 'target/tester-army-daemon',
  cleanupTimeout: 90_000,
  targets: config.targets.map(target => ({
    ...target,
    name: 'real-daemon-browser',
    app: {
      ...target.app,
      command: {
        ...target.app.command,
        log: 'target/tester-army-daemon-server.log',
        env: { UC_TESTER_DAEMON: '1', UC_UI_PLATFORM: 'macos' },
      },
    },
  })),
}
