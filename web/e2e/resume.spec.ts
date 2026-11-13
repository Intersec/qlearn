// A course that opens again comes back to the page that the learner read,
// not to the page that the agent pushed ahead of the learner.

import { expect, test } from '@playwright/test';
import { PAGE_1, PAGE_2 } from './agent.ts';
import { start, type Running } from './server.ts';

let server: Running;

test.beforeEach(async () => {
  server = await start();
  await server.agent.course('Raft elections');
});

test.afterEach(async () => {
  await server?.stop();
});

test('a new tab opens on the page read last, not on the page pushed last', async ({ browser }) => {
  const { agent } = server;
  await agent.page('1', PAGE_1);
  await agent.page('2', PAGE_2);

  // The learner reads page 2, and the agent prepares page 3 meanwhile.
  const first = await browser.newPage();
  await first.goto(server.url);
  await expect(first.getByRole('heading', { name: 'The vote', level: 1 })).toBeVisible();
  await agent.page('3', '# The term\n\nEach election starts a new term.\n');
  await expect(first.locator('.ready')).toContainText('Page 3 is ready');
  await first.close();

  // The URL that qlearn prints names no page, as when the course opens again.
  const again = await browser.newPage();
  await again.goto(server.url);

  await expect(again.getByRole('heading', { name: 'The vote', level: 1 })).toBeVisible();
  await expect(again).toHaveURL(/#page=2$/);
  // Next goes on to the page that waits.
  await again.getByRole('button', { name: 'Next' }).click();
  await expect(again.getByRole('heading', { name: 'The term', level: 1 })).toBeVisible();
  await again.close();
});
