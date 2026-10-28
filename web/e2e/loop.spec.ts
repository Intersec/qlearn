// The loop of step 1: an agent teaches a lesson of two pages through the page.
//
// This suite also watches the console. A page that works says nothing
// there, so an error or a warning through the whole loop fails the test.

import { expect, test, type Page } from '@playwright/test';
import { PAGE_1, PAGE_2 } from './agent.ts';
import { start, type Running } from './server.ts';

let server: Running;
let complaints: string[];

test.beforeEach(async ({ page }) => {
  complaints = [];
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') {
      complaints.push(`${message.type()}: ${message.text()}`);
    }
  });
  page.on('pageerror', (error) => complaints.push(String(error)));

  server = await start();
  await server.agent.course('Raft elections');
  await server.agent.page('1', PAGE_1);
});

test.afterEach(async ({ page }) => {
  // The page first: a page whose server stops says so in the console.
  await page.close();
  await server?.stop();
  expect(complaints).toEqual([]);
});

function title(page: Page) {
  return page.locator('article.lesson').getByRole('heading', { level: 1 });
}

function pill(page: Page) {
  return page.locator('.agent-pill');
}

test('an agent teaches a lesson of two pages', async ({ page }) => {
  const { agent } = server;
  await page.goto(server.url);

  const lesson = page.locator('article.lesson');
  await expect(title(page)).toHaveText('The election timer');
  await expect(lesson.locator('.md-doc').getByRole('table')).toContainText('becomes a candidate');
  await expect(page).toHaveURL(/#page=1$/);

  // The fence comes in colors: the server marks the tokens, the theme
  // colors them.
  const fence = lesson.locator('pre');
  await expect(fence).toContainText('int election_timeout(int base)');
  const keyword = fence.locator('.tok-storage, .tok-keyword').first();
  await expect(keyword).toBeVisible();
  const colors = await fence.evaluate((pre) => {
    const token = pre.querySelector('.tok-storage, .tok-keyword');
    return { pre: getComputedStyle(pre).color, token: token && getComputedStyle(token).color };
  });
  expect(colors.token).not.toBe(colors.pre);

  // The excerpt comes from the repository, at the commit of the push. Its
  // hidden line holds no text: the server keeps it.
  const excerpt = lesson.locator('.excerpt');
  await expect(excerpt.locator('.excerpt-path')).toHaveText('src/election.c:1-6');
  await expect(excerpt.locator('.excerpt-commit')).toHaveText(/^[0-9a-f]{7}$/);
  await expect(excerpt.locator('[data-line="3"]')).toHaveClass(/excerpt-marked/);
  await expect(excerpt.locator('[data-line="3"] .excerpt-text')).toHaveText(
    '    int timeout = base + rand() % base;',
  );
  await expect(excerpt.locator('[data-line="5"] .excerpt-hidden')).toBeVisible();
  // An empty line holds no character, and stays one line high.
  const empty = excerpt.locator('[data-line="4"]');
  expect(await empty.locator('.excerpt-text').evaluate((cell) => cell.textContent)).toBe('');
  const full = await excerpt.locator('[data-line="3"]').boundingBox();
  expect((await empty.boundingBox())?.height).toBe(full?.height);
  await expect(excerpt).not.toContainText('log_debug');

  const waited = agent.wait(0);
  await expect(pill(page)).toHaveText('Agent connected');
  await page.getByRole('button', { name: 'Next' }).click();

  // The request names the page that the learner left, so the agent needs
  // no second command.
  const { events, next } = await waited;
  expect(events).toEqual([
    expect.objectContaining({
      kind: 'next',
      author: 'learner',
      page: '1',
      detail: { left: { id: '1', title: 'The election timer' } },
    }),
  ]);

  await agent.status('Writing page 2: the vote');
  await expect(page.locator('.page-end')).toHaveText(
    'The agent prepares the next page: Writing page 2: the vote',
  );
  await expect(page.locator('.agent-status')).toHaveText('Writing page 2: the vote');

  // The agent pushes the page and waits again, as the procedure says. The
  // wait clears its status.
  await agent.page('2', PAGE_2);
  void agent.wait(next);
  await expect(pill(page)).toHaveText('Agent connected');
  await expect(page.locator('.agent-status')).toBeHidden();

  // The new page waits for the learner, who still reads page 1. Next opens
  // it now, with no second request.
  await expect(page.locator('.ready')).toContainText('Page 2 is ready: The vote.');
  await expect(title(page)).toHaveText('The election timer');
  await page.getByRole('button', { name: 'Next' }).click();

  await expect(title(page)).toHaveText('The vote');
  await expect(page).toHaveURL(/#page=2$/);
  await expect(page.locator('.ready')).toBeHidden();

  await page.goBack();
  await expect(title(page)).toHaveText('The election timer');
  await expect(page).toHaveURL(/#page=1$/);

  await page.goForward();
  await expect(title(page)).toHaveText('The vote');
  await expect(page).toHaveURL(/#page=2$/);

  await page.reload();
  await expect(title(page)).toHaveText('The vote');
  await expect(page).toHaveURL(/#page=2$/);
});

test('a page pushed while the learner reads does not move the learner', async ({ page }) => {
  await page.goto(server.url);
  await expect(title(page)).toHaveText('The election timer');

  await server.agent.page('2', PAGE_2);

  const ready = page.locator('.ready');
  await expect(ready).toContainText('Page 2 is ready: The vote.');
  await expect(title(page)).toHaveText('The election timer');
  await expect(page).toHaveURL(/#page=1$/);

  await ready.getByRole('link', { name: 'Read it' }).click();
  await expect(title(page)).toHaveText('The vote');
  await expect(page).toHaveURL(/#page=2$/);
  await expect(ready).toBeHidden();
});

test('the top bar says whether an agent listens, and whether it works', async ({ page }) => {
  const { agent } = server;
  await page.goto(server.url);
  await expect(title(page)).toHaveText('The election timer');
  await expect(pill(page)).toHaveText('No agent');

  const waited = agent.wait(0);
  await expect(pill(page)).toHaveText('Agent connected');

  await page.getByRole('button', { name: 'Next' }).click();
  const { next } = await waited;
  await expect(pill(page)).toHaveText('Agent working');

  await agent.status('Reading the vote');
  await expect(pill(page)).toHaveText('Agent working');
  await expect(page.locator('.page-end')).toHaveText(
    'The agent prepares the next page: Reading the vote',
  );

  // A wait with no page answers the request: the learner may ask again.
  void agent.wait(next);
  await expect(pill(page)).toHaveText('Agent connected');
  await expect(page.locator('.agent-status')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Next' })).toBeVisible();
});
