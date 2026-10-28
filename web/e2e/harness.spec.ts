// The helpers of the browser tests. A helper that leaks or lies makes every
// other spec say less, so they get tests of their own. No browser here.

import { expect, test } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { PAGE_1 } from './agent.ts';
import { build } from './fixture.ts';
import { start } from './server.ts';

function head(repo: string): string {
  return execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: repo,
    encoding: 'utf8',
    env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_INDEX_FILE: '' },
  }).trim();
}

test('the fixture ignores the git setup of the person who runs it', () => {
  const home = mkdtempSync(join(tmpdir(), 'qlearn-e2e-home-'));
  const saved = { ...process.env };
  try {
    // A hook that makes every commit unique, and an index of another
    // repository, as git exports it to a hook.
    const hooks = join(home, 'hooks');
    mkdirSync(hooks);
    writeFileSync(
      join(hooks, 'commit-msg'),
      '#!/bin/sh\necho "Change-Id: $$-$(date +%N)" >>"$1"\n',
    );
    chmodSync(join(hooks, 'commit-msg'), 0o755);
    writeFileSync(join(home, 'gitconfig'), `[core]\n\thooksPath = ${hooks}\n`);
    process.env.GIT_CONFIG_GLOBAL = join(home, 'gitconfig');
    process.env.GIT_INDEX_FILE = join(home, 'index');

    const one = build();
    const two = build();
    try {
      expect(head(one.repo)).toBe(head(two.repo));
      expect(existsSync(join(home, 'index'))).toBe(false);
    } finally {
      one.remove();
      two.remove();
    }
  } finally {
    process.env = saved;
    rmSync(home, { recursive: true, force: true });
  }
});

test('stop throws the whole repository away', async () => {
  for (let run = 0; run < 10; run++) {
    const server = await start();
    const base = dirname(server.fixture.repo);
    await server.agent.course('Raft elections');
    await server.agent.page('1', PAGE_1);
    void server.agent.wait(0);
    await server.stop();

    expect(existsSync(base), `run ${run} left ${base}`).toBe(false);
  }
});

test('a binary that cannot start fails the start, with the reason', async () => {
  await expect(start(join(tmpdir(), 'no-such-qlearn'))).rejects.toThrow(/make build/);
});
