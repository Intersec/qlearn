// The quiz format, the first part of step 3: the agent adds a quiz under a
// lesson, and the page shows it in its first state. The right option never
// reaches the page.

import { expect, test } from '@playwright/test';
import { PAGE_2, QUIZ_1, QUIZ_2 } from './agent.ts';
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
  await server.agent.page('1', PAGE_2);
});

test.afterEach(async ({ page }) => {
  // The page first: a page whose server stops says so in the console.
  await page.close();
  await server?.stop();
  expect(complaints).toEqual([]);
});

test('a quiz shows under its lesson, with no answer in the page', async ({ page }) => {
  const { agent } = server;
  const bodies: string[] = [];
  page.on('response', async (response) => {
    if (response.url().includes('/api/pages/')) {
      bodies.push(await response.text());
    }
  });
  await agent.quiz('1', QUIZ_1);
  await page.goto(server.url);

  // The open question comes alone: the choices would give it away.
  const first = page.locator('.quiz[data-quiz="1"]');
  await expect(first.locator('.quiz-open label')).toHaveText(QUIZ_1.open.prompt);
  await expect(first.locator('.quiz-choices')).toHaveCount(0);
  await first.getByRole('textbox').fill('Two followers rarely time out at once.');
  await expect(first.getByRole('button', { name: 'Send' })).toBeDisabled();

  // A quiz added while the learner reads shows at once, under the first.
  await agent.quiz('1', QUIZ_2);
  const second = page.locator('.quiz[data-quiz="2"]');
  await expect(second.locator('.quiz-header')).toHaveText(['Timer', 'Zero']);
  await expect(second.locator('.quiz-option').nth(1)).toHaveText('A number from 150 to 299');
  await expect(first.getByRole('textbox')).toHaveValue('Two followers rarely time out at once.');

  expect(bodies.length).toBeGreaterThan(1);
  for (const body of bodies) {
    expect(body).not.toContain('"answer"');
  }
});
