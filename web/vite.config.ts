import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  // Relative asset paths so the same build works under any static host path.
  base: './',
  build: {
    // Web Serial ships in Chrome/Edge 89; nothing older can use the page.
    target: 'chrome89',
    cssTarget: 'chrome89',
  },
  // The sample topology is read from the repository's topology/ folder.
  server: { host: '127.0.0.1', fs: { allow: ['..'] } },
  preview: { host: '127.0.0.1' },
  test: {
    projects: [
      // The page's logic, and its first paint rendered on the server.
      { extends: true, test: { name: 'page', include: ['src/**/*.test.ts'], exclude: ['src/**/*.svelte.test.ts'], setupFiles: ['src/wasm/test-setup.ts'] } },
      // Runes as the browser runs them ($effect, flushSync): Svelte's client build.
      { extends: true, resolve: { conditions: ['browser'] }, test: { name: 'runes', include: ['src/**/*.svelte.test.ts'], environment: './src/test-client-env.ts' } },
    ],
  },
});
