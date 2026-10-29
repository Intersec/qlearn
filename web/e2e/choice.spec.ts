// The choices of step 3: the agent asks the learner to choose, in place of
// a dialog of its terminal, at the end of a page or at the top of the
// syllabus column. The learner answers in the page, and the wait of the
// agent gets the labels of the picks.

import { expect, test } from '@playwright/test';
import { CHOICE_LEVEL, CHOICE_NEXT, PAGE_2 } from './agent.ts';
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

test('the learner answers a choice in the column and one on a page', async ({ page }) => {
  const { agent } = server;
  const first = agent.wait(0);
  await page.goto(server.url);
  await expect(page.getByRole('heading', { name: 'The vote' })).toBeVisible();

  // The agent asks while the learner reads: both show at once.
  expect(await agent.choose(CHOICE_LEVEL)).toEqual({ id: 'choice-1' });
  expect(await agent.choose(CHOICE_NEXT)).toEqual({ id: 'choice-2' });
  const column = page.getByRole('navigation', { name: 'Syllabus' }).locator('.choice');
  const onPage = page.locator('.lesson .choice');
  await expect(column).toHaveAttribute('data-choice', 'choice-1');
  await expect(onPage).toHaveAttribute('data-choice', 'choice-2');
  await expect(column.locator('.choice-header')).toHaveText('Level');
  await expect(column.locator('.choice-description').first()).toHaveText('I start from the paper');
  await expect(onPage.getByRole('radio')).toHaveCount(2);
  await expect(onPage.getByRole('checkbox')).toHaveCount(3);

  // The column: words of the learner beside a pick.
  const send = column.getByRole('button', { name: 'Send' });
  await expect(send).toBeDisabled();
  await column.getByRole('radio', { name: /Read the paper/ }).check();
  await column.getByRole('textbox', { name: 'Other' }).fill('Only the first half');
  await send.click();
  await expect(column).toHaveAttribute('data-answered', 'true');
  await expect(column.locator('.choice-pick')).toHaveText(['✓Read the paper']);

  const asked = await first;
  expect(asked.events).toEqual([
    {
      seq: expect.any(Number),
      kind: 'chosen',
      author: 'learner',
      page: null,
      id: 'choice-1',
      detail: {
        questions: [
          {
            ...CHOICE_LEVEL.questions[0],
            multi: false,
          },
        ],
        answers: [
          {
            header: 'Level',
            picked: [1],
            labels: ['Read the paper'],
            other: 'Only the first half',
          },
        ],
      },
    },
  ]);

  // The page: one pick, then two picks of a question that takes several.
  const second = agent.wait(asked.next);
  await onPage.getByRole('radio', { name: /A break/ }).check();
  await expect(onPage.getByRole('button', { name: 'Send' })).toBeDisabled();
  await onPage.getByRole('checkbox', { name: /The log/ }).check();
  await onPage.getByRole('checkbox', { name: /The timer/ }).check();
  await onPage.getByRole('button', { name: 'Send' }).click();
  await expect(onPage).toHaveAttribute('data-answered', 'true');

  const { events } = await second;
  expect(events.map((event) => [event.kind, event.page, event.id])).toEqual([
    ['chosen', '1', 'choice-2'],
  ]);
  expect((events[0].detail as { answers: unknown }).answers).toEqual([
    { header: 'Next step', picked: [1], labels: ['A break'], other: null },
    { header: 'Topics', picked: [0, 2], labels: ['The timer', 'The log'], other: null },
  ]);

  // A reload finds both answers, read only.
  await page.reload();
  await expect(column).toHaveAttribute('data-answered', 'true');
  await expect(onPage.locator('.choice-pick')).toHaveText(['✓A break', '✓The timer', '✓The log']);
  await expect(page.locator('.choice input')).toHaveCount(0);
});

test('an open choice of the column comes back after a reload', async ({ page }) => {
  const { agent } = server;
  await agent.choose(CHOICE_LEVEL);
  await page.goto(server.url);
  const column = page.getByRole('navigation', { name: 'Syllabus' }).locator('.choice');
  await expect(column).toHaveAttribute('data-answered', 'false');

  // Free text alone answers a question.
  const waited = agent.wait(0);
  await column.getByRole('textbox', { name: 'Other' }).fill('I wrote a toy one in Go');
  await page.reload();
  await expect(column).toHaveAttribute('data-answered', 'false');
  await column.getByRole('textbox', { name: 'Other' }).fill('I wrote a toy one in Go');
  await column.getByRole('button', { name: 'Send' }).click();
  await expect(column.locator('.choice-other-sent')).toContainText('I wrote a toy one in Go');

  const { events } = await waited;
  expect((events[0].detail as { answers: unknown }).answers).toEqual([
    { header: 'Level', picked: [], labels: [], other: 'I wrote a toy one in Go' },
  ]);
});
