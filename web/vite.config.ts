import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';

// The port of the Rust server in development. Vite proxies the API to it, so
// the interface runs with hot reload against the real server.
const SERVER_PORT = 7420;

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  build: {
    // The binary embeds this directory. See crates/qlearn/src/assets.rs.
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    proxy: { '/api': `http://127.0.0.1:${SERVER_PORT}` },
  },
});
