// The threads of step 3: the learner asks about a block with a quote, or
// about the whole page, the wait of the agent gets the question with the
// source of the block, and the reply of the agent shows in the page at
// once.

import { expect, test, type Page } from '@playwright/test';
import { PAGE_1 } from './agent.ts';
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

/** Select `text` in the page, as the learner does with the mouse. */
async function select(page: Page, text: string) {
  await page.evaluate((wanted) => {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const at = node.textContent?.indexOf(wanted) ?? -1;
      if (at >= 0) {
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + wanted.length);
        getSelection()?.removeAllRanges();
        getSelection()?.addRange(range);
        return;
      }
    }
    throw new Error(`no text ${wanted}`);
  }, text);
}

test('the learner asks about a block, and the reply of the agent shows live', async ({ page }) => {
  const { agent } = server;
  const waited = agent.wait(0);
  await page.goto(server.url);
  const anchor = page.locator('.thread-anchor', {
    has: page.getByRole('heading', { name: 'Why a random part' }),
  });
  const block = await anchor.locator(':scope > [data-block]').getAttribute('data-block');
  expect(block).toMatch(/^[0-9a-f]{12}$/);

  // The Ask button shows on hover, and the selection becomes the quote.
  await select(page, 'the votes would split');
  await anchor.hover();
  await anchor.getByRole('button', { name: 'Ask' }).click();
  const asking = anchor.locator('.asking');
  await expect(asking.locator('.thread-quote')).toHaveText('the votes would split');
  await expect(page).toHaveURL(new RegExp(`#page=1&block=${block}$`));
  await asking.getByRole('textbox').fill('Why would they split *again*?');
  await asking.getByRole('button', { name: 'Send' }).click();

  const thread = anchor.locator('.thread');
  await expect(thread.locator('.thread-quote')).toHaveText('the votes would split');
  await expect(thread.locator('.thread-comment[data-author="learner"]')).toContainText(
    'Why would they split again?',
  );
  await expect(thread.locator('.thread-waiting')).toHaveText('The agent writes its answer here.');
  await expect(thread.getByRole('textbox', { name: 'Reply' })).toHaveCount(0);
  await expect(asking).toHaveCount(0);

  // The agent gets the question with the source of its block.
  const { events, next } = await waited;
  expect(events).toEqual([
    {
      seq: expect.any(Number),
      kind: 'ask',
      author: 'learner',
      page: '1',
      id: '1-1',
      detail: {
        thread: {
          id: '1-1',
          page: '1',
          block,
          quote: 'the votes would split',
          comments: [
            { author: 'learner', body: 'Why would they split *again*?', at: expect.any(String) },
          ],
        },
        block: {
          id: block,
          kind: 'text',
          source: expect.stringMatching(/^## Why a random part\n\nWith one fixed timeout/),
        },
        earlier: false,
      },
    },
  ]);

  // The reply of the agent shows with no reload.
  const again = agent.wait(next);
  await agent.reply('1-1', 'Each follower times out at **the same** moment.');
  const reply = thread.locator('.thread-comment[data-author="agent"]');
  await expect(reply.locator('strong')).toHaveText('the same');
  await expect(thread.locator('.thread-waiting')).toHaveCount(0);

  // A reply of the learner asks again.
  await thread.getByRole('textbox', { name: 'Reply' }).fill('And with a random part?');
  await thread.getByRole('button', { name: 'Send' }).click();
  const asked = await again;
  expect(asked.events.map((event) => [event.kind, event.id])).toEqual([['ask', '1-1']]);

  // A reload comes back to the thread. A short window puts its block below
  // the fold, so only the scroll brings it in.
  await page.setViewportSize({ width: 1600, height: 300 });
  await page.reload();
  await expect(page).toHaveURL(new RegExp(`#page=1&block=${block}$`));
  await expect(thread.locator('.thread-comment')).toHaveCount(3);
  await expect(anchor.getByRole('heading', { name: 'Why a random part' })).toBeInViewport();
  expect(await page.locator('main.course').evaluate((pane) => pane.scrollTop)).toBeGreaterThan(0);
});

test('the learner asks about the whole page, and the reply shows live', async ({ page }) => {
  const { agent } = server;
  const waited = agent.wait(0);
  await page.goto(server.url);

  // The button sits at the end of the page, before Next.
  const button = page.getByRole('button', { name: 'Ask about this page' });
  const [ask, next] = await Promise.all([
    button.boundingBox(),
    page.getByRole('button', { name: 'Next' }).boundingBox(),
  ]);
  expect(ask!.y).toBeLessThan(next!.y);
  await button.click();
  const about = page.getByRole('region', { name: 'About this page' });
  await expect(about.locator('.threads-head')).toHaveText('About this page');
  await expect(page).toHaveURL(/#page=1&block=page$/);
  await about.getByRole('textbox', { name: 'Your question' }).fill('Where does a *term* start?');
  await about.getByRole('button', { name: 'Send' }).click();

  const thread = about.locator('.thread');
  await expect(thread.locator('.thread-comment[data-author="learner"]')).toContainText(
    'Where does a term start?',
  );
  await expect(thread.locator('.thread-quote')).toHaveCount(0);
  await expect(about.locator('.asking')).toHaveCount(0);
  await expect(button).toBeVisible();

  // The agent gets the question, with no block.
  const { events } = await waited;
  expect(events).toEqual([
    {
      seq: expect.any(Number),
      kind: 'ask',
      author: 'learner',
      page: '1',
      id: '1-1',
      detail: {
        thread: {
          id: '1-1',
          page: '1',
          block: null,
          quote: null,
          comments: [
            { author: 'learner', body: 'Where does a *term* start?', at: expect.any(String) },
          ],
        },
        block: null,
        earlier: false,
      },
    },
  ]);

  // The reply of the agent shows with no reload.
  await agent.reply('1-1', 'A candidate starts it, when its **timeout** fires.');
  await expect(thread.locator('.thread-comment[data-author="agent"] strong')).toHaveText('timeout');
  await expect(thread.locator('.thread-waiting')).toHaveCount(0);

  // A reload comes back to the thread, at the end of the page, with no box
  // open: the thread is there.
  await page.setViewportSize({ width: 1600, height: 300 });
  await page.reload();
  await expect(page).toHaveURL(/#page=1&block=page$/);
  await expect(thread.locator('.thread-comment')).toHaveCount(2);
  await expect(about.locator('.threads-head')).toBeInViewport();
  expect(await page.locator('main.course').evaluate((pane) => pane.scrollTop)).toBeGreaterThan(0);
  await expect(about.locator('.asking')).toHaveCount(0);
});

test('a thread whose block is gone moves to the end of the page', async ({ page }) => {
  const { agent } = server;
  await page.goto(server.url);
  const anchor = page.locator('.thread-anchor', {
    has: page.getByRole('heading', { name: 'Why a random part' }),
  });
  await expect(anchor).toBeVisible();
  await select(page, 'the votes would split');
  await anchor.hover();
  await anchor.getByRole('button', { name: 'Ask' }).click();
  await anchor.getByRole('textbox').fill('Why would they split?');
  await anchor.getByRole('button', { name: 'Send' }).click();
  await expect(anchor.locator('.thread')).toHaveCount(1);
  const whole = await agent.comment('1', 'We stopped at the random part.');
  await expect(page.locator('.threads-page .thread')).toHaveAttribute('data-thread', whole.id);

  await agent.page('1', PAGE_1.replace('the votes would split', 'the votes split'));

  const earlier = page.locator('.threads-earlier');
  await expect(earlier.locator('.threads-head')).toHaveText('On an earlier version of this page');
  await expect(earlier.locator('.thread-quote')).toHaveText('the votes would split');
  await expect(page.locator('.thread-anchor .thread')).toHaveCount(0);
  await expect(page.locator('.threads-page .thread')).toHaveCount(1);
});

test('the Ask button leaves the commit of an excerpt in reach', async ({ page }) => {
  await page.goto(server.url);
  const anchor = page.locator('.thread-anchor', { has: page.locator('.excerpt') });
  const commit = anchor.locator('.excerpt-commit');
  await expect(commit).toHaveText(/^[0-9a-f]{7}$/);

  // With no hover, the button stays in sight in quiet ink, in the corner
  // of the commit. On hover, it takes the color of the agent.
  const ask = anchor.getByRole('button', { name: 'Ask' });
  const colorOf = (token: string) =>
    page.evaluate((token) => {
      const probe = document.createElement('span');
      probe.style.color = `var(${token})`;
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    }, token);
  await expect(ask).toHaveCSS('opacity', '1');
  await expect(ask).toHaveCSS('color', await colorOf('--ink-quiet'));
  await anchor.hover();
  await expect(ask).toHaveCSS('color', await colorOf('--action'));

  // The tooltip of the commit holds the full hash: the pointer must reach it.
  const reached = await commit.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const hit = document.elementFromPoint(box.right - 1, box.top + box.height / 2);
    return hit === element;
  });
  expect(reached).toBe(true);
});
