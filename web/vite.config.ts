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
  server: { host: '127.0.0.1' },
  preview: { host: '127.0.0.1' },
  test: {
    include: ['src/**/*.test.ts'],
  },
});
