// The steps of step 4: the server checks the format, and the learner plays
// the steps one by one through the real binary, with the real Mermaid for
// a diagram step.
//
// This suite watches the console too: steps play with no error.

import { expect, test } from '@playwright/test';
import { PAGE_STEPS } from './agent.ts';
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

test('the learner plays the steps, and sees the cells that changed', async ({ page }) => {
  await server.agent.page('1', PAGE_STEPS);
  await page.goto(server.url);

  const steps = page.locator('figure.steps');
  const count = steps.locator('.steps-count [aria-hidden="true"]');
  const next = steps.getByRole('button', { name: 'Next step', exact: true });
  const previous = steps.getByRole('button', { name: 'Previous step', exact: true });
  const changed = steps.locator('td.steps-changed');

  // The first step shows at load, with nothing marked.
  await expect(steps.locator('.steps-label')).toHaveText('t = 0 ms');
  await expect(count).toHaveText('1 / 4');
  await expect(steps.locator('.steps-body em')).toHaveText('itself');
  await expect(changed).toHaveCount(0);

  // The second step marks the term and the vote of S2.
  await next.click();
  await expect(count).toHaveText('2 / 4');
  await expect(changed).toHaveCount(2);
  await expect(changed.first()).toContainText('(changed, was 5)');
  const weight = await changed.first().evaluate((cell) => getComputedStyle(cell).fontWeight);
  expect(Number(weight)).toBeGreaterThanOrEqual(700);

  // The third step is a diagram, drawn by Mermaid.
  await next.click();
  await expect(steps.locator('table')).toHaveCount(0);
  await expect(steps.locator('.diagram-svg svg')).toContainText('RequestVote');

  // The arrows play the steps while the view has the focus.
  await page.keyboard.press('ArrowRight');
  await expect(count).toHaveText('4 / 4');
  await expect(changed).toHaveCount(3);
  await page.keyboard.press('ArrowRight');
  await expect(count).toHaveText('4 / 4');
  await page.keyboard.press('Home');
  await expect(count).toHaveText('1 / 4');
  // The first step has no step before it.
  await expect(previous).toHaveAttribute('aria-disabled', 'true');

  // The view fits a narrow screen: a wide table scrolls in its own box.
  await page.setViewportSize({ width: 360, height: 800 });
  await expect
    .poll(() => steps.evaluate((figure) => figure.scrollWidth <= figure.clientWidth))
    .toBe(true);
});

test('steps that break a rule are refused with every fault', async () => {
  const wrong = PAGE_STEPS.replace('["S2", 5, "follower", ""]', '["S2", 5, "follower"]').replace(
    '"body": "S1 asks S3 too.",',
    '"body": "S1 asks S3 too.", "rows": [["S1", 6, "candidate", "S1"]],',
  );

  await expect(server.agent.page('1', wrong)).rejects.toThrow(
    /1\.md:6: steps: steps\[0\]\.rows\[1\]: holds 3 cells, and columns holds 4[^]*1\.md:6: steps: steps\[2\]: holds both rows and a diagram/,
  );
});
