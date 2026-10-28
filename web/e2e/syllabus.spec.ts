// The syllabus of step 2: the scripted agent writes it with `qlearn
// syllabus`, and the column at the left shows it and follows it live.
//
// This suite watches the console too, and expects it silent.

import { expect, test, type Page } from '@playwright/test';
import { PAGE_1, PAGE_2, SYLLABUS, type PlannedPart } from './agent.ts';
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
  await server.agent.page('1', PAGE_1);
});

test.afterEach(async ({ page }) => {
  // The page first: a page whose server stops says so in the console.
  await page.close();
  await server?.stop();
  expect(complaints).toEqual([]);
});

function entry(page: Page, id: string) {
  return page.locator(`.syllabus [data-page="${id}"]`);
}

function heading(page: Page) {
  return page.locator('article.lesson').getByRole('heading', { level: 1 });
}

test('the column shows the syllabus that the agent writes, and follows it', async ({ page }) => {
  const { agent } = server;
  await page.goto(server.url);
  await expect(heading(page)).toHaveText('The election timer');

  const column = page.getByRole('navigation', { name: 'Syllabus' });
  await expect(column.locator('.syllabus-part-title')).toHaveText(['The timer', 'The states']);
  await expect(column.locator('[data-page]')).toHaveCount(4);
  await expect(column.locator('.syllabus-title')).toHaveText([
    'The election timer',
    'The vote',
    'The states of a server',
    'Checkpoint',
  ]);
  await expect(entry(page, '1')).toHaveAttribute('data-state', 'taught');
  await expect(entry(page, '1').getByRole('link')).toHaveAttribute('aria-current', 'page');
  await expect(entry(page, '1').locator('.syllabus-latest')).toBeVisible();
  // A planned page is shown, and cannot be opened.
  await expect(entry(page, '2')).toHaveAttribute('data-state', 'planned');
  await expect(entry(page, '2').getByRole('link')).toHaveCount(0);

  // The agent pushes page 2: the column says so, and the learner stays.
  await agent.page('2', PAGE_2);
  await expect(entry(page, '2')).toHaveAttribute('data-state', 'taught');
  await expect(entry(page, '2').locator('.syllabus-latest')).toBeVisible();
  await expect(entry(page, '1').locator('.syllabus-latest')).toHaveCount(0);
  await expect(heading(page)).toHaveText('The election timer');

  await entry(page, '2').getByRole('link').click();
  await expect(heading(page)).toHaveText('The vote');
  await expect(page).toHaveURL(/#page=2$/);
  await expect(entry(page, '2')).toHaveAttribute('data-shown', 'true');
  await expect(entry(page, '1')).not.toHaveAttribute('data-shown');

  // A new syllabus renames and reorders the pages with content, and drops
  // a planned one. Each page keeps its state.
  const later: PlannedPart[] = [
    {
      title: 'The vote first',
      pages: [
        { id: '2', kind: 'lesson', title: 'One vote per term' },
        { id: '1', kind: 'lesson', title: 'The timer' },
      ],
    },
    { title: 'Then', pages: [{ id: '3', kind: 'lesson', title: 'The states of a server' }] },
  ];
  await agent.syllabus(later);
  await expect(column.locator('.syllabus-part-title')).toHaveText(['The vote first', 'Then']);
  await expect(column.locator('.syllabus-title')).toHaveText([
    'One vote per term',
    'The timer',
    'The states of a server',
  ]);
  await expect(column.locator('.syllabus-id')).toHaveText(['2', '1', '3']);
  await expect(entry(page, '1')).toHaveAttribute('data-state', 'taught');
  await expect(entry(page, '2')).toHaveAttribute('data-shown', 'true');

  // A syllabus that drops a page with content is refused, and changes
  // nothing on the screen.
  await expect(
    agent.syllabus([{ title: '', pages: [{ id: '2', kind: 'lesson', title: 'The vote' }] }]),
  ).rejects.toThrow(/page 1 \("The timer"\) has content/);
  await expect(column.locator('.syllabus-part-title')).toHaveText(['The vote first', 'Then']);
});

test('a page opened from the column shows from its top', async ({ page }) => {
  // As long as page 1, so the scroll of page 1 fits in page 2.
  await server.agent.page('2', PAGE_1.replace('# The election timer', '# The vote'));
  await page.setViewportSize({ width: 1200, height: 500 });
  await page.goto(`${server.url}#page=1`);
  await expect(heading(page)).toHaveText('The election timer');

  const course = page.locator('main.course');
  await course.evaluate((main) => main.scrollTo(0, main.scrollHeight));
  await expect(heading(page)).not.toBeInViewport();

  await entry(page, '2').getByRole('link').click();
  await expect(heading(page)).toHaveText('The vote');
  await expect(heading(page)).toBeInViewport();
  expect(await course.evaluate((main) => main.scrollTop)).toBe(0);
});

test('on a narrow screen, the syllabus opens over the page', async ({ page }) => {
  await server.agent.page('2', PAGE_2);
  await page.setViewportSize({ width: 600, height: 800 });
  await page.goto(server.url);
  await expect(heading(page)).toHaveText('The vote');

  const column = page.getByRole('navigation', { name: 'Syllabus' });
  const toggle = page.getByRole('button', { name: 'Syllabus' });
  await expect(column).toBeHidden();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');

  await toggle.click();
  await expect(column).toBeVisible();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');

  // Picking a page opens it, and gives the screen back to it.
  await entry(page, '1').getByRole('link').click();
  await expect(heading(page)).toHaveText('The election timer');
  await expect(column).toBeHidden();

  // A wide screen has room for both, and needs no button.
  await page.setViewportSize({ width: 1200, height: 800 });
  await expect(column).toBeVisible();
  await expect(toggle).toBeHidden();
});
