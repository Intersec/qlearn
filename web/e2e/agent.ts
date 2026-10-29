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
  /** The id of the server, for `--server` of the next wait. */
  server: string;
}

/**
 * The first page of the lesson: text, a table, a fence of C, callouts, and
 * an excerpt.
 */
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

> [!hook]
> A random wait breaks the tie: the first follower to wake up wins.

> [!pitfall]
> A \`base\` of 0 makes \`rand() % base\` divide by zero:
>
> - the follower crashes at its first timeout;
> - the cluster then loses one voter.

> [!teaser]
> Lesson 3 shows what the leader does with its term.

## In the repository

\`\`\`excerpt
path: src/election.c
lines: 1-6
mark: 3
hide: 5
\`\`\`

The marked line draws the random part.
`;

/** The second page of the lesson. */
export const PAGE_2 = `# The vote

A candidate asks every other server for its vote. A server gives one vote
per term, to the first candidate that asks.
`;

/** A diagram that Mermaid refuses: the arrow leads nowhere. */
export const BROKEN_DIAGRAM = 'graph LR\n  Follower -->\n';

/** A page with a diagram that Mermaid draws, and one that it refuses. */
export const PAGE_DIAGRAMS = `# The states of a server

A server is a follower, a candidate, or the leader. It changes state on a
timeout or on a message.

\`\`\`mermaid
flowchart LR
  F[Follower] -- times out --> C[Candidate]
  C -- wins the vote --> L[Leader]
  C -- hears a leader --> F
  L -- hears a higher term --> F
\`\`\`

## One vote

\`\`\`mermaid
${BROKEN_DIAGRAM}\`\`\`
`;

/** The same page, once the agent fixed the diagram that Mermaid refused. */
export const PAGE_DIAGRAMS_FIXED = PAGE_DIAGRAMS.replace(
  BROKEN_DIAGRAM,
  `sequenceDiagram
  participant C as Candidate
  participant S as Server
  C->>S: RequestVote(term 6)
  S-->>C: vote granted
`,
);

/** A page with a bar chart of two series and a line chart with a gap. */
export const PAGE_CHARTS = `# How long an election takes

A wider range of timeouts ends an election sooner.

\`\`\`chart
{
  "kind": "bar",
  "title": "Election time, by timeout range",
  "x": { "label": "Timeout range (ms)", "values": ["150-300", "150-200", "150-155", "150-151"] },
  "y": { "label": "Median election time", "unit": "ms" },
  "series": [
    { "name": "5 servers", "values": [283, 410, 1100, 1520] },
    { "name": "9 servers", "values": [301, 455, 1290, 1985] }
  ]
}
\`\`\`

## The terms

S1 was down at second 4, so its line has a gap there.

\`\`\`chart
{
  "kind": "line",
  "title": "The term of each server, second by second",
  "x": { "label": "Time (s)", "values": [0, 1, 2, 3, 4, 5, 6, 7, 8] },
  "y": { "label": "Term" },
  "series": [
    { "name": "S1", "values": [1, 1, 2, 2, null, 4, 4, 5, 5] },
    { "name": "S2", "values": [1, 2, 2, 3, 3, 4, 4, 5, 6] },
    { "name": "S3", "values": [1, 1, 2, 3, 4, 4, 5, 5, 6] }
  ]
}
\`\`\`
`;

/** A page with steps: two tables, a diagram step, then a table with a row more. */
export const PAGE_STEPS = `# Two candidates

S1 and S2 time out close together. Stop on step 2, and guess what S1
does next.

\`\`\`steps
{
  "title": "Two candidates in term 6",
  "columns": ["Server", "Term", "State", "Voted for"],
  "steps": [
    { "label": "t = 0 ms", "body": "S1 times out first, and votes for *itself*.",
      "rows": [["S1", 6, "candidate", "S1"], ["S2", 5, "follower", ""], ["S3", 5, "follower", ""]] },
    { "label": "t = 2 ms", "body": "S2 gets the \`RequestVote\` of S1, and takes term 6.",
      "rows": [["S1", 6, "candidate", "S1"], ["S2", 6, "follower", "S1"], ["S3", 5, "follower", ""]] },
    { "label": "t = 3 ms", "body": "S1 asks S3 too.",
      "diagram": "sequenceDiagram\\n  S1->>S3: RequestVote(term 6)\\n  S3-->>S1: vote granted\\n" },
    { "label": "t = 4 ms", "body": "S1 has a majority, and leads.",
      "rows": [["S1", 6, "leader", "S1"], ["S2", 6, "follower", "S1"], ["S3", 6, "follower", "S1"]] }
  ]
}
\`\`\`
`;

/** A page with an SVG and a PNG of the repository. */
export const PAGE_IMAGES = `# The states of a server

The design docs draw the three states of a server, and what moves a
server from one to the next:

![The three states of a server: follower, candidate and leader, with the event that moves a server between them](docs/states.svg)

A follower that hears nothing for a while becomes a candidate.

![Six blue pixels](docs/pixels.png)
`;

/** A quiz of the first page: an open question, then a choice. */
export const QUIZ_1 = {
  open: {
    prompt: 'In 2 sentences: why is the election timeout random?',
    points: ['random-timeout'],
  },
  choices: [
    {
      header: 'Split vote',
      prompt: 'Two candidates get 2 votes each out of 5. What happens next?',
      options: [
        'The candidate with the higher id wins',
        'Both time out and start a *new* election in a new term',
        'The leader of the previous term breaks the tie',
      ],
      answer: 1,
      point: 'split-vote',
    },
  ],
  points: {
    'random-timeout': { label: 'Why the election timeout is random' },
    'split-vote': { label: 'How a split vote ends' },
  },
};

/** A second quiz of the first page, of choices only, on a known point. */
export const QUIZ_2 = {
  choices: [
    {
      header: 'Timer',
      prompt: 'What does election_timeout(150) return?',
      options: ['Exactly 150', 'A number from 150 to 299', 'A number from 0 to 149'],
      answer: 1,
      point: 'random-timeout',
    },
    {
      header: 'Zero',
      prompt: 'What does a base of 0 do?',
      options: ['It divides by zero', 'It disables the timer'],
      answer: 0,
      point: 'random-timeout',
    },
  ],
};

/**
 * The debrief of the first quiz: what the learner got, the code that
 * explains the rest, and a diagram of it.
 */
export const DEBRIEF_1 = `# The split vote

You named the random part, but not what it buys: the first follower to
wake up asks for the votes before the others start.

\`\`\`excerpt
path: src/election.c
lines: 1-6
mark: 3
\`\`\`

\`\`\`mermaid
sequenceDiagram
  participant S1
  participant S2
  S1->>S2: RequestVote(term 6)
  S2-->>S1: vote granted
\`\`\`
`;

/**
 * The cheatsheet of the course: a hook, a glossary, a pitfall, a diagram
 * and an excerpt.
 */
export const SHEET = `# Raft elections on one page

> [!hook]
> A random wait breaks the tie: the first follower to wake up wins.

## Glossary

| Name | What it is |
|---|---|
| \`election_timeout()\` | The wait of a follower: a base and a random part |
| term | A number that only grows; one leader at most per term |
| split vote | No candidate gets a majority; the term ends with no leader |

> [!pitfall]
> A \`base\` of 0 makes \`rand() % base\` divide by zero.

## The states

\`\`\`mermaid
flowchart LR
  F[Follower] -- times out --> C[Candidate]
  C -- wins the vote --> L[Leader]
  C -- hears a leader --> F
\`\`\`

## The timer

\`\`\`excerpt
path: src/election.c
lines: 1-6
mark: 3
\`\`\`
`;

/** The level at the start, asked at the top of the syllabus column. */
export const CHOICE_LEVEL = {
  questions: [
    {
      header: 'Level',
      prompt: 'How well do you know Raft?',
      options: [
        { label: 'New to it', description: 'I start from the paper' },
        { label: 'Read the paper', description: 'I know the terms, not the code' },
        { label: 'Wrote one', description: 'I want the details of this code base' },
      ],
    },
  ],
};

/** The next step, asked at the end of the first page. */
export const CHOICE_NEXT = {
  page: '1',
  questions: [
    {
      header: 'Next step',
      prompt: 'The timer is done. What next?',
      options: [
        { label: 'The vote', description: 'How a candidate gets a majority' },
        { label: 'A break', description: 'The course waits here' },
      ],
      multi: false,
    },
    {
      header: 'Topics',
      prompt: 'What should the follow-up quiz cover?',
      options: [
        { label: 'The timer', description: '' },
        { label: 'The vote', description: '' },
        { label: 'The log', description: 'Not taught yet' },
      ],
      multi: true,
    },
  ],
};

/** A part of the syllabus, as the agent writes it. */
export interface PlannedPart {
  title: string;
  pages: { id: string; kind: 'lesson' | 'checkpoint' | 'followup'; title: string }[];
}

/** The syllabus of the course of the tests: the pages above, and one more. */
export const SYLLABUS: PlannedPart[] = [
  {
    title: 'The timer',
    pages: [
      { id: '1', kind: 'lesson', title: 'The election timer' },
      { id: '2', kind: 'lesson', title: 'The vote' },
    ],
  },
  {
    title: 'The states',
    pages: [
      { id: '3', kind: 'lesson', title: 'The states of a server' },
      { id: 'c1', kind: 'checkpoint', title: 'Checkpoint' },
    ],
  },
];

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

  /** Write the syllabus from a file, as the procedure says. */
  async syllabus(parts: PlannedPart[]) {
    const path = join(this.fixture.work, 'syllabus.json');
    writeFileSync(path, JSON.stringify({ parts }, null, 2));
    await this.run(['syllabus', '--file', path]);
  }

  /** Push a page from a file, as the procedure says. */
  async page(id: string, source: string) {
    const path = join(this.fixture.work, `${id}.md`);
    writeFileSync(path, source);
    await this.run(['page', id, '--file', path]);
  }

  /** Add a quiz under a page from a file, as the procedure says. */
  async quiz(id: string, quiz: object) {
    const path = join(this.fixture.work, `quiz-${id}.json`);
    writeFileSync(path, JSON.stringify(quiz, null, 2));
    await this.run(['quiz', id, '--file', path]);
  }

  /**
   * Write the debrief of the quiz `n` of a page from a file, with a grade
   * for each point of its open question, as the procedure says.
   */
  async debrief(id: string, n: number, source: string, points: Record<string, string> = {}) {
    const path = join(this.fixture.work, `debrief-${id}-${n}.md`);
    writeFileSync(path, source);
    const grades = Object.entries(points).flatMap(([point, grade]) => [
      '--point',
      `${point}=${grade}`,
    ]);
    await this.run(['debrief', id, String(n), '--file', path, ...grades]);
  }

  /**
   * Open a thread, as the procedure says: `target` is a page, or a page,
   * `#` and a block. Answers the thread.
   */
  async comment(target: string, body: string): Promise<{ id: string }> {
    return JSON.parse(await this.run(['comment', target, '--body', body])) as { id: string };
  }

  /**
   * Ask the learner to choose from a file, as the procedure says. Answers
   * the id of the choice.
   */
  async choose(choice: object): Promise<{ id: string }> {
    const path = join(this.fixture.work, 'choice.json');
    writeFileSync(path, JSON.stringify(choice, null, 2));
    return JSON.parse(await this.run(['choose', '--file', path])) as { id: string };
  }

  /** Write a finding, or replace the one with its id, as the procedure says. */
  async finding(id: string, title: string, body: string, checked = false) {
    const flag = checked ? ['--checked'] : [];
    await this.run(['finding', id, '--title', title, '--body', body, ...flag]);
  }

  /** Write the cheatsheet from a file, as the procedure says. */
  async cheatsheet(source: string) {
    const path = join(this.fixture.work, 'cheatsheet.md');
    writeFileSync(path, source);
    await this.run(['cheatsheet', '--file', path]);
  }

  /** Mark the course finished, as the end of the procedure does. */
  async finish() {
    await this.run(['course', '--status', 'finished']);
  }

  /** Write in a thread, as the procedure says. */
  async reply(thread: string, body: string) {
    await this.run(['reply', thread, '--body', body]);
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
