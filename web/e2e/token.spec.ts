// The token: a request that does not carry it gets nothing.

import { expect, test } from '@playwright/test';
import { start } from './server.ts';

test('a request without the token gets 401, with the error in JSON', async ({ page, request }) => {
  const server = await start();
  try {
    // The address with no query: the token stays behind.
    const session = new URL('/api/session', server.url).href;

    const refused = await request.get(session);
    expect(refused.status()).toBe(401);
    expect(await refused.json()).toEqual({
      error: { code: 'unauthorized', message: expect.any(String) },
    });

    // The same request from the page, which the token let in.
    await page.goto(server.url);
    const allowed = await page.request.get(session);
    expect(allowed.status()).toBe(200);
  } finally {
    await server.stop();
  }
});
