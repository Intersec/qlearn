import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

// Separate from vite.config.ts on purpose: a test run has no use for the
// Tailwind plugin or the dev proxy.
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  test: {
    include: ['src/**/*.test.ts'],
    // Node by default. A file that needs a DOM asks for one on its first
    // line: // @vitest-environment jsdom
    environment: 'node',
  },
});
