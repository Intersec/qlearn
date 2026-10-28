import { readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';

// The port of the Rust server in development. Vite proxies the API to it, so
// the interface runs with hot reload against the real server.
const SERVER_PORT = 7420;

// Where `make dev` runs the server: the worktree of this file.
const ADDRESS = fileURLToPath(new URL('../.learn/server.json', import.meta.url));

// The page that Vite serves never sees the token, so the proxy hands the
// cookie of the server to every API call. The file is read again each time,
// because a restarted server has a new token.
function serverCookie(): string | undefined {
  try {
    const { port, token } = JSON.parse(readFileSync(ADDRESS, 'utf8'));
    return `qlearn_token_${port}=${token}`;
  } catch {
    return undefined;
  }
}

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
    proxy: {
      '/api': {
        target: `http://127.0.0.1:${SERVER_PORT}`,
        configure: (proxy) => {
          proxy.on('proxyReq', (request) => {
            const cookie = serverCookie();
            if (cookie) {
              request.setHeader('cookie', cookie);
            }
            // The server refuses a write from another origin, and the
            // origin of Vite is another one.
            request.removeHeader('origin');
          });
        },
      },
    },
  },
});
