// The quiz of step 3: the agent adds a quiz under a lesson, the learner
// answers it in its steps, and the wait of the agent gets the answer. The
// right option never reaches the page before the learner sends the choices.

import { expect, test } from '@playwright/test';
import { PAGE_1, PAGE_2, QUIZ_1, QUIZ_2 } from './agent.ts';
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
  await expect(first.getByRole('button', { name: 'Send' })).toBeDisabled();
  await first.getByRole('textbox').fill('Two followers rarely time out at once.');
  await expect(first.getByRole('button', { name: 'Send' })).toBeEnabled();

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

test('the learner answers a quiz, and the agent gets the answer', async ({ page }) => {
  const { agent } = server;
  const bodies: string[] = [];
  page.on('response', async (response) => {
    if (response.url().includes('/api/pages/')) {
      bodies.push(await response.text());
    }
  });
  await agent.page('1', PAGE_1);
  await agent.quiz('1', QUIZ_1);
  const waited = agent.wait(0);
  await page.goto(server.url);
  const quiz = page.locator('.quiz[data-quiz="1"]');
  const entry = page.locator('.syllabus-page[data-page="1"]');
  const hidden = page.locator('.excerpt .excerpt-hidden');
  await expect(entry).toHaveAttribute('data-state', 'taught');
  await expect(hidden).toHaveCount(1);

  // 1. The open question, alone.
  await quiz.getByRole('textbox').fill('Two followers rarely time out at once.');
  await quiz.getByRole('button', { name: 'Send' }).click();

  // 2. The choices, each with an Other box. The answer is still unknown.
  await expect(quiz).toHaveAttribute('data-step', 'choices');
  await expect(quiz.locator('.quiz-sent-text')).toHaveText(
    'Two followers rarely time out at once.',
  );
  await expect(quiz.locator('.quiz-header')).toHaveText('Split vote');
  for (const body of bodies) {
    expect(body).not.toContain('"answer"');
  }
  const send = quiz.getByRole('button', { name: 'Send' });
  await expect(send).toBeDisabled();
  await quiz.getByRole('radio').first().check();
  await quiz.getByRole('textbox', { name: 'Other' }).fill('Unless the ids tie too');
  await send.click();

  // 3. Right or wrong at once, from the server, and the agent writes the
  // debrief.
  await expect(quiz).toHaveAttribute('data-step', 'graded');
  await expect(quiz.locator('.quiz-verdict')).toHaveText('Wrong');
  await expect(quiz.locator('.quiz-option[data-mark="right"]')).toContainText('Both time out');
  await expect(quiz.locator('.quiz-option[data-mark="wrong"]')).toContainText('higher id');
  await expect(quiz.locator('.quiz-debrief')).toHaveText('The agent writes the debrief.');
  // Every quiz of the page is answered: the page is, and the hidden line
  // shows.
  await expect(entry).toHaveAttribute('data-state', 'answered');
  await expect(hidden).toHaveCount(0);
  await expect(page.locator('.excerpt [data-line="5"]')).toContainText('log_debug');

  const { events } = await waited;
  expect(events).toEqual([
    {
      seq: expect.any(Number),
      kind: 'answer',
      author: 'learner',
      page: '1',
      id: '1',
      detail: {
        quiz: 1,
        open: {
          prompt: QUIZ_1.open.prompt,
          points: ['random-timeout'],
          text: 'Two followers rarely time out at once.',
        },
        choices: [
          {
            ...QUIZ_1.choices[0],
            picked: 0,
            ok: false,
            other: 'Unless the ids tie too',
          },
        ],
      },
    },
  ]);

  // A reload shows the quiz where the learner left it.
  await page.reload();
  await expect(quiz).toHaveAttribute('data-step', 'graded');
  await expect(quiz.locator('.quiz-other-sent')).toContainText('Unless the ids tie too');
});
