// The cheatsheet of step 4: the agent writes it with `qlearn cheatsheet`,
// the server adds the map of the code and the points to revisit, and the
// page lays it out for an A4 sheet, on the screen and in print.
//
// This suite watches the console too: the cheatsheet draws with no error.

import { expect, test, type Page } from '@playwright/test';
import { DEBRIEF_1, PAGE_1, QUIZ_1, SHEET, SYLLABUS } from './agent.ts';
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
  await server.agent.syllabus(SYLLABUS);
});

test.afterEach(async ({ page }) => {
  // The page first: a page whose server stops says so in the console.
  await page.close();
  await server?.stop();
  expect(complaints).toEqual([]);
});

/**
 * A lesson whose quiz the learner answers with a wrong choice, then a
 * debrief that grades the open answer partial.
 */
async function taught(page: Page) {
  const { agent } = server;
  await agent.page('1', PAGE_1);
  await agent.quiz('1', QUIZ_1);
  await page.goto(server.url);
  const quiz = page.locator('.quiz[data-quiz="1"]');
  await quiz.getByRole('textbox').fill('So that two followers rarely start at once.');
  await quiz.getByRole('button', { name: 'Send' }).click();
  await quiz.getByRole('radio').first().check();
  await quiz.getByRole('button', { name: 'Send' }).click();
  await quiz.locator('.quiz-debrief').waitFor();
  await agent.debrief('1', 1, DEBRIEF_1, { 'random-timeout': 'partial' });
  await page.locator('.syllabus-page[data-state="debriefed"]').waitFor();
}

test('the cheatsheet adds the map of the code and the points to revisit', async ({ page }) => {
  await taught(page);
  const entry = page.locator('.syllabus-sheet');
  await expect(entry.locator('[aria-disabled="true"]')).toHaveText(/Cheatsheet/);

  await server.agent.cheatsheet(SHEET);
  await server.agent.finish();

  // The column opens it, and so does the note of a finished course.
  await expect(entry.getByRole('link', { name: 'Cheatsheet' })).toBeVisible();
  const note = page.locator('.sheet-ready');
  await expect(note).toContainText('Cheatsheet ready');
  await note.getByRole('link', { name: 'Open it' }).click();
  await expect(page).toHaveURL(/#cheatsheet$/);
  await expect(note).toHaveCount(0);

  const sheet = page.getByRole('article', { name: 'Cheatsheet' });
  await expect(sheet.getByRole('heading', { name: 'Raft elections on one page' })).toBeVisible();
  await expect(sheet.locator('.callout')).toHaveCount(2);
  await expect(sheet.locator('.diagram-svg svg')).toBeVisible();
  // The lesson and its debrief show the same lines: one range, one page.
  await expect(sheet.locator('.cheatsheet-range')).toHaveCount(1);
  await expect(sheet.locator('.cheatsheet-where')).toHaveText('src/election.c:1-6');
  await expect(sheet.locator('.cheatsheet-range .cheatsheet-lesson')).toHaveText(
    '1 The election timer',
  );
  await expect(sheet.locator('.cheatsheet-point .cheatsheet-label')).toHaveText([
    'How a split vote ends',
    'Why the election timeout is random',
  ]);
  await expect(sheet.locator('.cheatsheet-result')).toHaveText(['Missed', 'Partial']);
  await expect(sheet.locator('.cheatsheet-keep')).toContainText('.learn/myproject/');
  await expect(sheet.locator('.ask')).toHaveCount(0);

  // A reload comes back to it, and a lesson that it names opens.
  await page.reload();
  await expect(sheet.locator('.cheatsheet-range')).toBeVisible();
  await sheet.locator('.cheatsheet-range .cheatsheet-lesson').click();
  await expect(page).toHaveURL(/#page=1$/);
  await expect(page.getByRole('heading', { name: 'The election timer' })).toBeVisible();
  await page.goBack();
  await expect(sheet).toBeVisible();
});

test('the page under the cheatsheet opens from its top', async ({ page }) => {
  await server.agent.page('1', PAGE_1);
  await server.agent.cheatsheet(SHEET);
  await page.setViewportSize({ width: 1200, height: 400 });
  await page.goto(`${server.url}#page=1`);
  const heading = page.getByRole('heading', { name: 'The election timer' });
  await expect(heading).toBeVisible();

  await page.locator('.syllabus-sheet').getByRole('link', { name: 'Cheatsheet' }).click();
  const sheet = page.getByRole('article', { name: 'Cheatsheet' });
  await sheet.locator('.cheatsheet-range').waitFor();
  const course = page.locator('main.course');
  await course.evaluate((main) => main.scrollTo(0, main.scrollHeight));
  expect(await course.evaluate((main) => main.scrollTop)).toBeGreaterThan(0);

  // The page keeps its id under the sheet: the move back still scrolls.
  await page.locator('.syllabus-page[data-page="1"]').getByRole('link').click();
  await expect(heading).toBeInViewport();
  expect(await course.evaluate((main) => main.scrollTop)).toBe(0);
});

test('print shows the sheet alone, in two columns, with light colors', async ({ page }) => {
  await server.agent.page('1', PAGE_1);
  await server.agent.cheatsheet(SHEET);
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto(`${server.url}#cheatsheet`);
  const sheet = page.locator('.cheatsheet-sheet');
  await sheet.locator('.diagram-svg svg').waitFor();
  const look = () =>
    page.evaluate(() => ({
      columns: getComputedStyle(document.querySelector('.cheatsheet-sheet')!).columnCount,
      ground: getComputedStyle(document.body).backgroundColor,
    }));
  expect(await look()).toEqual({ columns: '2', ground: 'rgb(19, 20, 22)' });
  await expect(page.locator('button:visible')).not.toHaveCount(0);

  await page.emulateMedia({ media: 'print', colorScheme: 'dark' });

  expect(await look()).toEqual({ columns: '2', ground: 'rgb(255, 255, 255)' });
  await expect(page.locator('button:visible')).toHaveCount(0);
  await expect(page.getByRole('navigation', { name: 'Syllabus' })).toBeHidden();
  await expect(page.locator('.top-bar')).toBeHidden();
  await expect(sheet.locator('.cheatsheet-range')).toBeVisible();
  // Mermaid drew the diagram dark: print turns it light.
  await expect(sheet.locator('.diagram')).toHaveAttribute('data-scheme', 'dark');
  expect(
    await sheet.locator('.diagram-svg').evaluate((svg) => getComputedStyle(svg).filter),
  ).toContain('invert(1)');
});
