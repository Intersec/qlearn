// The chart of step 4: the server checks the format, and the page draws
// it in SVG with the colors of its theme, through the real binary.
//
// This suite watches the console too: a chart draws with no error.

import { expect, test } from '@playwright/test';
import { PAGE_CHARTS } from './agent.ts';
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

test('a bar chart and a line chart draw in the colors of the theme', async ({ page }) => {
  await server.agent.page('1', PAGE_CHARTS);
  await page.goto(server.url);

  const charts = page.getByRole('img', { name: /chart/ });
  await expect(charts).toHaveCount(2);
  const [bar, line] = [page.locator('.chart').first(), page.locator('.chart').nth(1)];
  await expect(bar.locator('.chart-bar')).toHaveCount(8);
  await expect(bar.locator('.chart-legend li')).toHaveText(['5 servers', '9 servers']);
  await expect(line.locator('.chart-line')).toHaveCount(3);

  // The colors come from the tokens of the theme.
  const fill = () =>
    bar
      .locator('.chart-bar')
      .first()
      .evaluate((shape) => getComputedStyle(shape).fill);
  expect(await fill()).toBe('rgb(42, 120, 214)');
  await page.emulateMedia({ colorScheme: 'dark' });
  expect(await fill()).toBe('rgb(57, 135, 229)');

  // The chart fits a narrow screen, with no scroll of its own.
  await page.setViewportSize({ width: 360, height: 800 });
  await expect
    .poll(() => bar.evaluate((figure) => figure.scrollWidth <= figure.clientWidth))
    .toBe(true);
  const svg = await bar.locator('svg').boundingBox();
  const figure = await bar.boundingBox();
  expect(svg!.width).toBeLessThanOrEqual(figure!.width);
});

test('a chart that breaks a rule is refused with every fault', async () => {
  const wrong = PAGE_CHARTS.replace('[283, 410, 1100, 1520]', '[283, 410]').replace(
    '"kind": "line"',
    '"kind": "pie"',
  );

  await expect(server.agent.page('1', wrong)).rejects.toThrow(
    /1\.md:5: chart: series\[0\]\.values: holds 2 values, and x holds 4[^]*1\.md:22: chart: kind: takes "bar" or "line", not "pie"/,
  );
});
