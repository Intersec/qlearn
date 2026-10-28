import { accessSync, constants } from 'node:fs';
import { delimiter, join } from 'node:path';
import { defineConfig } from '@playwright/test';

/*
 * The browser is the one already on the machine. Playwright can download its
 * own, but a 150 MB download in every clone and every pipeline buys nothing
 * here: the interface has to work in the browser the reader uses.
 *
 * QLEARN_BROWSER names one outright. Otherwise the names below are looked up
 * on the PATH, as scripts/check.sh does, so the gate and this config agree.
 */
const NAMES = ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser'];

function executable(path: string): boolean {
  try {
    accessSync(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

// No browser is an error here: Playwright would look for its own download.
function browser(): string {
  const named = process.env.QLEARN_BROWSER;
  if (named && executable(named)) {
    return named;
  }

  const dirs = (process.env.PATH ?? '').split(delimiter).filter(Boolean);
  for (const name of NAMES) {
    for (const dir of dirs) {
      const path = join(dir, name);
      if (executable(path)) {
        return path;
      }
    }
  }
  throw new Error('no browser: set QLEARN_BROWSER, or put chrome or chromium on the PATH');
}

const found = browser();

export default defineConfig({
  testDir: './e2e',
  outputDir: './e2e/.results',
  fullyParallel: true,
  workers: 2,
  reporter: process.env.CI ? 'line' : 'list',
  timeout: 30_000,
  expect: { timeout: 5_000 },
  use: {
    // No sandbox: a container runs as root, and the course is local anyway.
    launchOptions: {
      executablePath: found,
      args: ['--no-sandbox', '--disable-dev-shm-usage'],
    },
    viewport: { width: 1600, height: 1000 },
    screenshot: 'only-on-failure',
  },
});
