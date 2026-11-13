// The Copy button of a block: the learner takes the text or the code of a
// block into the clipboard, and nothing of a quiz comes with it.

import { expect, test } from '@playwright/test';
import { PAGE_1 } from './agent.ts';
import { start, type Running } from './server.ts';

let server: Running;

test.use({ permissions: ['clipboard-read', 'clipboard-write'] });

test.beforeEach(async () => {
  server = await start();
  await server.agent.course('Raft elections');
  await server.agent.page('1', PAGE_1);
});

test.afterEach(async ({ page }) => {
  await page.close();
  await server?.stop();
});

const clipboard = (page: import('@playwright/test').Page) =>
  page.evaluate(() => navigator.clipboard.readText());

test('Copy takes the code of an excerpt, with its hidden line still empty', async ({ page }) => {
  await page.goto(server.url);
  const anchor = page.locator('.thread-anchor', { has: page.locator('.excerpt') });

  // By its class: its name changes once it copied.
  const copy = anchor.locator('.copy');
  await expect(copy).toHaveAccessibleName('Copy');
  await copy.click();

  await expect(copy).toHaveText('Copied');
  expect(await clipboard(page)).toBe(
    [
      'int election_timeout(int base)',
      '{',
      '    int timeout = base + rand() % base;',
      '',
      '',
      '    return timeout;',
    ].join('\n'),
  );
  // The label comes back, for the next copy.
  await expect(copy).toHaveText('Copy');
});

test('Copy takes the text of a block, as the page shows it', async ({ page }) => {
  await page.goto(server.url);
  const anchor = page.locator('.thread-anchor', {
    has: page.getByRole('heading', { name: 'Why a random part' }),
  });

  await anchor.getByRole('button', { name: 'Copy' }).click();

  const text = await clipboard(page);
  expect(text).toContain('Why a random part');
  expect(text).toContain('the votes would split again and again.');
  expect(text).not.toContain('<');
});
