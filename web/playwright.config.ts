import path from 'node:path'
import { defineConfig } from '@playwright/test'

const root = path.resolve(import.meta.dirname, '..')
const port = 18080

export default defineConfig({
  testDir: './tests',
  outputDir: './test-results',
  fullyParallel: false,
  workers: 1,
  timeout: 45_000,
  expect: { timeout: 10_000 },
  reporter: 'list',
  use: { baseURL: `http://127.0.0.1:${port}`, trace: 'retain-on-failure' },
  projects: [
    { name: 'desktop', use: { browserName: 'chromium', viewport: { width: 1440, height: 1000 } } },
    { name: 'mobile', use: { browserName: 'chromium', viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 1 } },
  ],
  webServer: {
    command: 'cargo run',
    cwd: root,
    url: `http://127.0.0.1:${port}/api/health`,
    reuseExistingServer: false,
    timeout: 120_000,
    env: {
      HOMECLIP_BIND: `127.0.0.1:${port}`,
      HOMECLIP_DATA: path.join(root, '.local', `e2e-${Date.now()}`),
      HOMECLIP_WEB: path.join(root, 'web', 'dist'),
    },
  },
})
