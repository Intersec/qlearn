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

/** The course of every fixture. */
export const SLUG = 'myproject';

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

function write(repo: string, path: string, content: string | Buffer) {
  const full = join(repo, path);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, content);
}

/** A figure of the design docs: the states of a server. */
export const STATES_SVG = `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="520" height="150" viewBox="0 0 520 150"
     font-family="sans-serif" font-size="15">
  <defs>
    <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7"
            orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="#3c4043"/>
    </marker>
  </defs>
  <rect x="10" y="50" width="120" height="50" rx="8" fill="#e8f0fe" stroke="#1a73e8"/>
  <text x="70" y="80" text-anchor="middle">Follower</text>
  <rect x="200" y="50" width="120" height="50" rx="8" fill="#fef7e0" stroke="#f9ab00"/>
  <text x="260" y="80" text-anchor="middle">Candidate</text>
  <rect x="390" y="50" width="120" height="50" rx="8" fill="#e6f4ea" stroke="#1e8e3e"/>
  <text x="450" y="80" text-anchor="middle">Leader</text>
  <path d="M 130 75 L 198 75" stroke="#3c4043" marker-end="url(#arrow)"/>
  <text x="164" y="66" text-anchor="middle" font-size="11">times out</text>
  <path d="M 320 75 L 388 75" stroke="#3c4043" marker-end="url(#arrow)"/>
  <text x="354" y="66" text-anchor="middle" font-size="11">wins</text>
  <path d="M 450 100 Q 260 150 70 100" fill="none" stroke="#3c4043" marker-end="url(#arrow)"/>
  <text x="260" y="140" text-anchor="middle" font-size="11">hears a higher term</text>
</svg>
`;

/**
 * An SVG with a script, which a page must never run. Drawn from an <img>,
 * it shows a box. Run, it would mark the window.
 */
export const TRAP_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60">
  <script>window.top.trapped = true; document.title = 'trapped';</script>
  <rect x="1" y="1" width="198" height="58" fill="#fce8e6" stroke="#d93025"/>
  <text x="100" y="35" text-anchor="middle" font-family="sans-serif">No script runs</text>
</svg>
`;

/** A PNG of 3 by 2 blue pixels: the bytes of the Rust fixture. */
export const PIXELS_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAIAAAASFvFNAAAAEElEQVR4nGOQKn4BQQxwFgBSrAi/OdW81wAAAABJRU5ErkJggg==',
  'base64',
);

/**
 * One commit of a small C file, the code that the course talks about, and
 * the figures of its docs.
 */
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
  write(repo, 'docs/states.svg', STATES_SVG);
  write(repo, 'docs/trap.svg', TRAP_SVG);
  write(repo, 'docs/pixels.png', PIXELS_PNG);
  git(repo, ['add', '-A']);
  git(repo, ['commit', '--quiet', '-m', 'Add the timer']);

  return {
    repo,
    work,
    remove: () => rmSync(base, { recursive: true, force: true }),
  };
}
