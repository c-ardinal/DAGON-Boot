import { defineConfig } from '@playwright/test';

// The Console in a real browser, on the development build's simulator (`npm run e2e`).
export default defineConfig({
  testDir: './e2e',
  timeout: 120_000,
  workers: 1,
  outputDir: process.env.DAGON_E2E_OUT ?? 'test-results',
  reporter: 'line',
  use: { baseURL: 'http://127.0.0.1:5199', video: 'off', screenshot: 'only-on-failure', channel: process.env.DAGON_E2E_CHANNEL },
  webServer: { command: 'npx vite --port 5199 --strictPort', url: 'http://127.0.0.1:5199', reuseExistingServer: true },
});
