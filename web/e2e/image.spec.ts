// The image of step 4: the server reads it in the repository and copies it
// into the store, and the page draws it from an <img>, through the real
// binary.
//
// This suite watches the console too: an image draws with no error.

import { expect, test } from '@playwright/test';
import { PAGE_IMAGES } from './agent.ts';
import { PIXELS_PNG } from './fixture.ts';
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

/** Whether every image of the page loaded, with a size. */
function drawn(page: import('@playwright/test').Page) {
  return page
    .locator('.image img')
    .evaluateAll((images) =>
      images.every(
        (image) =>
          (image as HTMLImageElement).complete && (image as HTMLImageElement).naturalWidth > 0,
      ),
    );
}

test('an SVG and a PNG of the repository draw with their caption', async ({ page }) => {
  await server.agent.page('1', PAGE_IMAGES);
  await page.goto(server.url);

  const images = page.locator('.image');
  await expect(images).toHaveCount(2);
  await expect.poll(() => drawn(page)).toBe(true);
  await expect(images.first().getByRole('img')).toHaveAttribute('alt', /three states/);
  await expect(images.first().locator('.image-path')).toHaveText('docs/states.svg');
  await expect(images.nth(1).locator('.image-commit')).toHaveText(/^[0-9a-f]{7}$/);

  // The bytes come back as they are in the repository, with their type.
  const src = await images.nth(1).locator('img').getAttribute('src');
  const response = await page.request.get(new URL(src!, server.url).href);
  expect(response.headers()['content-type']).toBe('image/png');
  expect(response.headers()['x-content-type-options']).toBe('nosniff');
  expect(Buffer.from(await response.body())).toEqual(PIXELS_PNG);

  // An image never runs wider than the page.
  await page.setViewportSize({ width: 360, height: 800 });
  const svg = await images.first().locator('img').boundingBox();
  const figure = await images.first().boundingBox();
  expect(svg!.width).toBeLessThanOrEqual(figure!.width);
});

test('an SVG runs no script, in the page or in a tab of its own', async ({ page, context }) => {
  await server.agent.page('1', '# A trap\n\n![A box that runs no script](docs/trap.svg)\n');
  await page.goto(server.url);
  await expect.poll(() => drawn(page)).toBe(true);

  expect(await page.evaluate(() => 'trapped' in window)).toBe(false);
  await expect(page).not.toHaveTitle('trapped');

  // Opened alone, the sandbox of the route stops its script. That tab is
  // not watched: the browser reports the blocked script there.
  const src = await page.locator('.image img').getAttribute('src');
  const alone = await context.newPage();
  await alone.goto(new URL(src!, server.url).href);
  await expect(alone.locator('svg rect')).toHaveCount(1);
  expect(await alone.evaluate(() => 'trapped' in window)).toBe(false);
  await expect(alone).not.toHaveTitle('trapped');
  await alone.close();
});

test('an image that is not a file of the repository is refused with its line', async () => {
  const wrong = PAGE_IMAGES.replace('docs/pixels.png', 'https://example.com/pixels.png').replace(
    'docs/states.svg',
    'docs/gone.svg',
  );

  await expect(server.agent.page('1', wrong)).rejects.toThrow(
    /1\.md:6: image: docs\/gone\.svg is not a file at commit [0-9a-f]{12}[^]*1\.md:10: image: "https:\/\/example\.com\/pixels\.png" is a URL/,
  );
});
