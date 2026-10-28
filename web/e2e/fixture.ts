// A real repository for the course to teach.
//
// The same rule as the Rust fixtures: real git commands, a fixed author and
// a fixed clock, and a temporary directory that goes away with the test run.
// The course lives in `.learn/` of that repository, so nothing of the person
// who runs the tests is touched.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';

export interface Fixture {
  repo: string;
  /**
   * Where the scripted agent writes the sources of its pages. Outside the
   * repository, as the notes of a real agent would be.
   */
  work: string;
  remove(): void;
}

// Both dates are fixed, so a commit has the same hash on every run.
const DATE = '2026-01-01T00:00:00+00:00';

// Git exports these to a hook. Inherited, they point git at another
// repository.
const ELSEWHERE = [
  'GIT_DIR',
  'GIT_WORK_TREE',
  'GIT_INDEX_FILE',
  'GIT_OBJECT_DIRECTORY',
  'GIT_ALTERNATE_OBJECT_DIRECTORIES',
  'GIT_COMMON_DIR',
];

/**
 * The environment of every child of the tests, git and qlearn alike. The
 * fixture must not depend on the machine: a global hook or a rerere would
 * change what it builds, as the Rust fixtures say.
 */
export function env(extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  const out: NodeJS.ProcessEnv = { ...process.env };
  for (const name of ELSEWHERE) {
    delete out[name];
  }
  return {
    ...out,
    LC_ALL: 'C',
    GIT_CONFIG_GLOBAL: '/dev/null',
    GIT_CONFIG_NOSYSTEM: '1',
    ...extra,
  };
}

function git(repo: string, args: string[]): string {
  return execFileSync('git', args, {
    cwd: repo,
    encoding: 'utf8',
    env: env({ GIT_AUTHOR_DATE: DATE, GIT_COMMITTER_DATE: DATE }),
  }).trim();
}

function write(repo: string, path: string, content: string) {
  const full = join(repo, path);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, content);
}

/** One commit of a small C file: the code that the course talks about. */
export function build(): Fixture {
  const base = mkdtempSync(join(tmpdir(), 'qlearn-e2e-'));
  const repo = join(base, 'repo');
  const work = join(base, 'work');
  mkdirSync(repo);
  mkdirSync(work);

  git(repo, ['init', '--quiet', '--initial-branch=main', '.']);
  git(repo, ['config', 'user.name', 'Test Author']);
  git(repo, ['config', 'user.email', 'author@example.com']);
  git(repo, ['config', 'commit.gpgsign', 'false']);
  git(repo, ['config', 'core.autocrlf', 'false']);

  write(
    repo,
    'src/election.c',
    [
      'int election_timeout(int base)',
      '{',
      '    int timeout = base + rand() % base;',
      '',
      '    log_debug("next election in %d ms", timeout);',
      '    return timeout;',
      '}',
      '',
    ].join('\n'),
  );
  git(repo, ['add', '-A']);
  git(repo, ['commit', '--quiet', '-m', 'Add the timer']);

  return {
    repo,
    work,
    remove: () => rmSync(base, { recursive: true, force: true }),
  };
}
