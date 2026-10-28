// The agent of a course, played by a script.
//
// It drives the course with the real commands of the binary, run as child
// processes in the repository, as an agent in a terminal runs them. Nothing
// in the tool changes for it.

import { execFile, execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { env, type Fixture } from './fixture.ts';

/**
 * The release binary, where cargo builds it. CARGO_TARGET_DIR or a
 * build.target-dir moves it, and a stale binary left in ./target would then
 * pass the tests in place of the current code.
 */
function binary(): string {
  const root = join(import.meta.dirname, '..', '..');
  const out = execFileSync(
    process.env.CARGO ?? 'cargo',
    ['metadata', '--format-version', '1', '--no-deps'],
    { cwd: root, encoding: 'utf8' },
  );
  const { target_directory } = JSON.parse(out) as { target_directory: string };
  return join(target_directory, 'release', 'qlearn');
}

export const BINARY = binary();

/** Stop a child and wait until it is gone, so it writes nothing after. */
export async function halt(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null || child.pid === undefined) {
    return;
  }
  const gone = new Promise<void>((resolve) => child.once('exit', () => resolve()));
  child.kill('SIGTERM');
  const timer = setTimeout(() => child.kill('SIGKILL'), 5000);
  await gone;
  clearTimeout(timer);
}

/** A request of the learner, as `qlearn wait` prints it. */
export interface Request {
  seq: number;
  kind: string;
  author: 'learner' | 'agent';
  page: string | null;
  id: string | null;
  detail?: unknown;
}

/** What `qlearn wait` prints. */
export interface Waited {
  events: Request[];
  next: number;
  reset?: boolean;
}

/** The first page of the lesson: text, a table and a fence of C. */
export const PAGE_1 = `# The election timer

A follower that hears no leader for a while starts an election. The
**timeout** decides how long it waits, and \`election_timeout\` computes it.

| Role | Waits for | Then |
|---|---|---|
| Follower | a heartbeat | becomes a candidate |
| Candidate | a majority of votes | becomes the leader |

\`\`\`c
/* A random part, so two followers rarely start at once. */
int election_timeout(int base)
{
    int timeout = base + rand() % base;

    log_debug("next election in %d ms", timeout);
    return timeout;
}
\`\`\`

## Why a random part

With one fixed timeout, every follower would start an election at the same
time, and the votes would split again and again.
`;

/** The second page of the lesson. */
export const PAGE_2 = `# The vote

A candidate asks every other server for its vote. A server gives one vote
per term, to the first candidate that asks.
`;

export class Agent {
  private readonly fixture: Fixture;
  private readonly waits = new Set<ChildProcess>();

  constructor(fixture: Fixture) {
    this.fixture = fixture;
  }

  /** Run one command of the agent and return what it prints. */
  run(args: string[]): Promise<string> {
    return new Promise((resolve, reject) => {
      execFile(
        BINARY,
        args,
        { cwd: this.fixture.repo, env: env({ NO_COLOR: '1' }) },
        (error, stdout, stderr) => {
          if (error) {
            reject(new Error(`qlearn ${args.join(' ')}: ${error.message}\n${stderr}`));
          } else {
            resolve(stdout);
          }
        },
      );
    });
  }

  async course(title: string) {
    await this.run(['course', '--title', title]);
  }

  /** Push a page from a file, as the procedure says. */
  async page(id: string, source: string) {
    const path = join(this.fixture.work, `${id}.md`);
    writeFileSync(path, source);
    await this.run(['page', id, '--file', path]);
  }

  async status(text: string) {
    await this.run(['status', text]);
  }

  /**
   * `qlearn wait` in the background, as the procedure runs it. The promise
   * ends when a request comes, and fails when the time runs out.
   */
  wait(after: number, timeout = 60): Promise<Waited> {
    const child = spawn(BINARY, ['wait', '--after', String(after), '--timeout', String(timeout)], {
      cwd: this.fixture.repo,
      env: env({ NO_COLOR: '1' }),
    });
    this.waits.add(child);

    const waited = new Promise<Waited>((resolve, reject) => {
      let out = '';
      let err = '';
      child.stdout.on('data', (chunk) => (out += String(chunk)));
      child.stderr.on('data', (chunk) => (err += String(chunk)));
      child.on('error', reject);
      child.on('close', (code) => {
        this.waits.delete(child);
        if (code === 0) {
          resolve(JSON.parse(out) as Waited);
        } else {
          reject(new Error(`qlearn wait stopped with ${code}:\n${out}${err}`));
        }
      });
    });
    // A wait that the end of a test kills is no failure of that test. A
    // caller that awaits the promise still sees it fail.
    waited.catch(() => {});
    return waited;
  }

  /** Kill the waits that still run: a wait outlives a server on purpose. */
  async stop() {
    const waits = [...this.waits];
    this.waits.clear();
    await Promise.all(waits.map(halt));
  }
}
