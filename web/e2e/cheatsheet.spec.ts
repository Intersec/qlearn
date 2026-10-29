// The cheatsheet of step 4: the agent writes it with `qlearn cheatsheet`,
// the server adds the map of the code and the points to revisit, and the
// page lays it out for an A4 sheet, on the screen and in print.
//
// This suite watches the console too: the cheatsheet draws with no error.

import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { expect, test, type Page } from '@playwright/test';
import { DEBRIEF_1, PAGE_1, QUIZ_1, SHEET, SHEET_FULL, SYLLABUS } from './agent.ts';
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

test('the button saves the cheatsheet as Markdown, with the cookie of the page', async ({
  page,
}) => {
  await server.agent.page('1', PAGE_1);
  await server.agent.cheatsheet(SHEET);
  await page.goto(`${server.url}#cheatsheet`);

  const saved = page.waitForEvent('download');
  await page.getByRole('link', { name: 'Save as Markdown' }).click();
  const download = await saved;

  expect(download.suggestedFilename()).toBe('myproject-cheatsheet.md');
  const text = await readFile(await download.path(), 'utf8');
  expect(text).toMatch(/^# Raft elections on one page\n/);
  expect(text).toContain('\n## Map of the code\n');
  expect(text).toContain('\n## To revisit\n');
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
  await expect(page.locator('.cheatsheet-markdown')).toBeHidden();
  await expect(page.getByRole('navigation', { name: 'Syllabus' })).toBeHidden();
  await expect(page.locator('.top-bar')).toBeHidden();
  await expect(sheet.locator('.cheatsheet-range')).toBeVisible();
  // Mermaid drew the diagram dark: print turns it light.
  await expect(sheet.locator('.diagram')).toHaveAttribute('data-scheme', 'dark');
  expect(
    await sheet.locator('.diagram-svg').evaluate((svg) => getComputedStyle(svg).filter),
  ).toContain('invert(1)');
});

test('a wide diagram spans both columns, and a long line of code wraps', async ({ page }) => {
  await server.agent.page('1', PAGE_1);
  await server.agent.cheatsheet(SHEET);
  await page.goto(`${server.url}#cheatsheet`);
  const sheet = page.locator('.cheatsheet-sheet');
  const svg = sheet.locator('.diagram-svg svg');
  await svg.waitFor();

  const look = () =>
    page.evaluate(() => {
      const anchors = [...document.querySelectorAll('.cheatsheet-sheet > .thread-anchor')];
      const of = (text: string) =>
        getComputedStyle(anchors.find((anchor) => anchor.textContent?.trim() === text)!).columnSpan;
      const drawn = document.querySelector<SVGSVGElement>('.cheatsheet .diagram-svg svg')!;
      return {
        heading: of('The states'),
        diagram: getComputedStyle(drawn.closest('.thread-anchor')!).columnSpan,
        // The 14px text of Mermaid, at the scale that the page draws it.
        text: (14 * drawn.getBoundingClientRect().width) / drawn.viewBox.baseVal.width,
        code: getComputedStyle(document.querySelector('.cheatsheet .excerpt-text')!).whiteSpace,
      };
    });

  const screen = await look();
  expect(screen).toMatchObject({ heading: 'all', diagram: 'all', code: 'pre-wrap' });
  expect(screen.text).toBeGreaterThanOrEqual(11);

  await page.setViewportSize({ width: 794, height: 1123 });
  await page.emulateMedia({ media: 'print' });
  const printed = await look();
  expect(printed).toMatchObject({ heading: 'all', diagram: 'all', code: 'pre-wrap' });
  expect(printed.text).toBeGreaterThanOrEqual(11);
});

test('the button saves one HTML file that stands alone, light, in two columns', async ({
  page,
  browser,
}) => {
  await server.agent.page('1', PAGE_1);
  await server.agent.cheatsheet(SHEET_FULL);
  // The page is dark, and the file is light all the same.
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto(`${server.url}#cheatsheet`);
  const sheet = page.locator('.cheatsheet-sheet');
  await expect(sheet.locator('.diagram-svg svg')).toHaveCount(2);
  await expect(sheet.locator('.steps-step')).toHaveCount(3);

  const saved = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Save as HTML' }).click();
  const download = await saved;
  expect(download.suggestedFilename()).toBe('myproject-cheatsheet.html');
  // The name ends in .html, so the browser opens the file as a page.
  const path = test.info().outputPath(download.suggestedFilename());
  await download.saveAs(path);
  const html = await readFile(path, 'utf8');
  expect(html).not.toMatch(/<script/i);
  expect(html).not.toMatch(/\son\w+=/i);
  expect(html).not.toMatch(/https?:/i);
  expect(html).not.toContain('/api/');
  expect(html).not.toContain('var(--series');

  // The file alone, offline, in a browser that prefers dark.
  const context = await browser.newContext({ offline: true, colorScheme: 'dark' });
  const alone = await context.newPage();
  const loaded: string[] = [];
  alone.on('request', (request) => loaded.push(request.url()));
  const url = pathToFileURL(path).href;
  await alone.goto(url);

  const file = alone.locator('article.cheatsheet');
  await expect(file.getByRole('heading', { name: 'Raft elections on one page' })).toBeVisible();
  await expect(file.locator('.diagram-svg svg')).toHaveCount(2);
  await expect(file.locator('.diagram-svg svg').first()).toBeVisible();
  await expect(file.locator('.chart .chart-bar').first()).toBeVisible();
  await expect(file.locator('.steps-step')).toHaveCount(3);
  await expect(file.locator('.excerpt .excerpt-path')).toHaveText('src/election.c:1-6');
  await expect(file.locator('.cheatsheet-range')).toHaveCount(1);
  await expect(file.locator('#cheatsheet-revisit')).toHaveText('To revisit');
  const look = await alone.evaluate(() => {
    const image = document.querySelector<HTMLImageElement>('.image img')!;
    return {
      columns: getComputedStyle(document.querySelector('.cheatsheet-sheet')!).columnCount,
      ground: getComputedStyle(document.body).backgroundColor,
      image: image.complete && image.naturalWidth,
      bar: getComputedStyle(document.querySelector('.chart-bar')!).fill,
      diagram: document.querySelector('.diagram-svg svg style')?.textContent ?? '',
    };
  });
  expect(look.columns).toBe('2');
  expect(look.ground).toBe('rgb(255, 255, 255)');
  expect(look.image).toBe(3);
  // The light color of the first series, written into the SVG.
  expect(look.bar).toBe('rgb(42, 120, 214)');
  // Mermaid drew it again with the light colors of the page.
  expect(look.diagram).toContain('#eef4fe');
  expect(look.diagram).not.toContain('#17202e');
  // Nothing loaded but the file and its data: URIs.
  expect(loaded.filter((one) => one !== url && !one.startsWith('data:'))).toEqual([]);

  await alone.emulateMedia({ media: 'print' });
  expect(
    await alone.evaluate(
      () => getComputedStyle(document.querySelector('.cheatsheet-sheet')!).columnCount,
    ),
  ).toBe('2');
  await context.close();
});
