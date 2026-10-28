// The diagram of step 2: the page draws it with the real Mermaid, and a
// source that Mermaid refuses reaches the wait of the agent as a request.
//
// This suite watches the console too: a broken diagram is an error of the
// agent, not of the page, and the console stays quiet.

import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test, type Page } from '@playwright/test';
import { BROKEN_DIAGRAM, PAGE_2, PAGE_DIAGRAMS, PAGE_DIAGRAMS_FIXED } from './agent.ts';
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
});

test.afterEach(async ({ page }) => {
  // The page first: a page whose server stops says so in the console.
  await page.close();
  await server?.stop();
  expect(complaints).toEqual([]);
});

/** The fill of the first node that Mermaid drew. */
function fill(page: Page): Promise<string> {
  return page
    .locator('.diagram-svg svg .node')
    .first()
    .locator('rect, path, polygon')
    .first()
    .evaluate((shape) => getComputedStyle(shape).fill);
}

test('a diagram that Mermaid refuses reaches the agent, which fixes it', async ({ page }) => {
  const { agent } = server;
  await agent.page('1', PAGE_DIAGRAMS);
  const waited = agent.wait(0);
  await page.goto(server.url);

  const drawn = page.locator('.diagram-svg svg');
  await expect(drawn).toHaveCount(1);
  await expect(drawn).toContainText('Candidate');
  await expect(drawn.locator('script, foreignObject')).toHaveCount(0);

  // The page shows the error in place of the diagram, with its source.
  const broken = page.locator('.diagram-broken');
  await expect(broken.locator('.diagram-source')).toHaveText(BROKEN_DIAGRAM.trimEnd());
  const error = await broken.locator('.diagram-error').textContent();
  expect(error).toBeTruthy();

  // The agent gets the block, its source and the error, with no second
  // command.
  const { events, next } = await waited;
  expect(events).toEqual([
    expect.objectContaining({
      kind: 'broken',
      author: 'learner',
      page: '1',
      detail: {
        block: {
          id: expect.stringMatching(/^[0-9a-f]{12}$/),
          kind: 'diagram',
          source: BROKEN_DIAGRAM,
        },
        error,
      },
    }),
  ]);
  const { id } = (events[0].detail as { block: { id: string } }).block;
  await expect(broken.locator('xpath=ancestor::section')).toHaveAttribute('data-block', id);

  // A dark theme draws the diagrams again, in its own colors. The agent
  // already knows of the broken one, and hears nothing more.
  const light = await fill(page);
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect.poll(() => fill(page)).not.toBe(light);
  await expect(broken).toBeVisible();
  await expect(agent.wait(next, 2)).rejects.toThrow(/stopped with 1/);

  // The agent fixes the source and pushes the page again.
  await agent.page('1', PAGE_DIAGRAMS_FIXED);
  await expect(drawn).toHaveCount(2);
  await expect(drawn.nth(1)).toContainText('RequestVote');
  await expect(broken).toHaveCount(0);
});

test('Mermaid loads only once a page holds a diagram', async ({ page }) => {
  const { agent } = server;
  const loaded: string[] = [];
  page.on('request', (request) => loaded.push(request.url()));
  await agent.page('1', PAGE_2);
  await page.goto(server.url);
  await expect(page.locator('article.lesson h1')).toHaveText('The vote');

  expect(loaded.filter((url) => url.includes('mermaid'))).toEqual([]);

  await agent.page('2', PAGE_DIAGRAMS_FIXED);
  await page.locator('.ready').getByRole('link', { name: 'Read it' }).click();
  await expect(page.locator('.diagram-svg svg')).toHaveCount(2);

  expect(loaded.some((url) => /\/assets\/mermaid\.core-[^/]*\.js$/.test(url))).toBe(true);
});

/** A server on another port of the loopback: another origin. */
async function elsewhere(): Promise<{ url: string; asked: string[]; close(): Promise<void> }> {
  const asked: string[] = [];
  const outside: Server = createServer((request, response) => {
    asked.push(request.url ?? '');
    response.writeHead(200, { 'content-type': 'image/svg+xml' });
    response.end('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>');
  });
  await new Promise<void>((resolve) => outside.listen(0, '127.0.0.1', resolve));
  const { port } = outside.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${port}`,
    asked,
    close: () => new Promise((resolve) => outside.close(() => resolve())),
  };
}

test('a diagram loads nothing from another origin', async ({ page }) => {
  const { agent } = server;
  const outside = await elsewhere();
  try {
    // Mermaid fetches the image of a shape while it draws, and a style
    // reaches the SVG as written.
    await agent.page(
      '1',
      `# Two pictures

\`\`\`mermaid
flowchart LR
  A@{ img: "${outside.url}/shape.svg", label: "A", pos: "t", w: 60, h: 60, constraint: "on" }
  A --> B
\`\`\`

## A style

\`\`\`mermaid
stateDiagram-v2
  classDef far fill:url(${outside.url}/fill.svg#a),filter:url(${outside.url}/filter.svg#f),mask:url(${outside.url}/mask.svg#m)
  [*] --> Idle
  class Idle far
\`\`\`
`,
    );
    const waited = agent.wait(0);
    await page.goto(server.url);

    // The image shape cannot load its image, so the agent hears of it.
    const { events } = await waited;
    expect(events).toEqual([expect.objectContaining({ kind: 'broken', page: '1' })]);
    await expect(page.locator('.diagram-svg svg')).toHaveCount(1);

    expect(outside.asked).toEqual([]);
    // The browser says what it refused, and only that. Chrome refuses a
    // filter of another origin by itself.
    expect(complaints.length).toBeGreaterThan(0);
    for (const complaint of complaints) {
      expect(complaint).toMatch(/Content Security Policy|Domains, protocols and ports must match/);
    }
    complaints = [];
  } finally {
    await outside.close();
  }
});
