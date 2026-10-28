// Screenshots of the interface, for a person or an agent to look at.
//
// `make shots` writes them under e2e/.shots. They are not a test: nothing
// passes or fails here, it only makes the interface visible.

import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import config from '../playwright.config.ts';
import { PAGE_1, PAGE_2, PAGE_DIAGRAMS, PAGE_DIAGRAMS_FIXED } from './agent.ts';
import { start } from './server.ts';

const OUT = join(import.meta.dirname, '.shots');
mkdirSync(OUT, { recursive: true });

// The browser of the tests, set up as they set it up.
const browser = await chromium.launch(config.use?.launchOptions);

for (const scheme of ['light', 'dark'] as const) {
  // A server per theme: the loop moves the course forward, and each theme
  // walks it from the start.
  const server = await start();
  const { agent } = server;
  const page = await browser.newPage({
    viewport: { width: 1600, height: 1000 },
    colorScheme: scheme,
  });
  const complaints: string[] = [];
  page.on('response', (response) => {
    if (response.status() >= 400) {
      complaints.push(`${response.status()} ${response.url()}`);
    }
  });
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') {
      complaints.push(`${message.type()}: ${message.text()}`);
    }
  });

  try {
    await agent.course('Raft elections');
    await agent.page('1', PAGE_1);
    const waited = agent.wait(0);

    // The first page, with an agent that listens.
    await page.goto(server.url);
    await page.locator('.agent-pill', { hasText: 'Agent connected' }).waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-page.png`) });

    // An excerpt of the repository, with a marked line and a hidden one.
    const excerpt = page.locator('.excerpt');
    await excerpt.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-excerpt.png`) });

    // The callouts, each with its marker.
    await page.locator('.callout').first().scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-callouts.png`) });

    // Next, and the agent at work on the page after.
    await page.getByRole('button', { name: 'Next' }).click();
    const { next } = await waited;
    await agent.status('Writing page 2: the vote');
    const end = page.locator('.page-end', { hasText: 'Writing page 2' });
    await end.waitFor();
    await end.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-preparing.png`) });

    // The page is ready, and the learner still reads the first one. The
    // agent waits again, as the procedure says, so the page shows it there.
    await agent.page('2', PAGE_2);
    void agent.wait(next);
    await page.locator('.ready').waitFor();
    await page.locator('.agent-pill', { hasText: 'Agent connected' }).waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-ready.png`) });

    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByRole('heading', { name: 'The vote' }).waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-page-2.png`) });

    // A diagram that Mermaid draws, and one that it refuses: the page shows
    // the error with the source, and sends it to the agent.
    await agent.page('3', PAGE_DIAGRAMS);
    await page.locator('.ready').getByRole('link', { name: 'Read it' }).click();
    await page.locator('.diagram-svg svg').waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-diagram.png`) });
    const broken = page.locator('.diagram-broken');
    await broken.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-diagram-broken.png`) });

    // The agent fixed it.
    await agent.page('3', PAGE_DIAGRAMS_FIXED);
    await broken.waitFor({ state: 'detached' });
    await page.locator('.diagram-svg svg').nth(1).scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-diagram-fixed.png`) });
  } finally {
    if (complaints.length) {
      console.log(`${scheme}: the page complained\n  ${complaints.join('\n  ')}`);
    }
    await page.close();
    await server.stop();
  }
}

await browser.close();
console.log(`screenshots in ${OUT}`);
