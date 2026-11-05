// A recording of a course, for the README.
//
// `make demo` runs the real binary on the repository of the browser tests,
// plays the agent with its real commands, and plays the learner in a
// browser. It writes `web/e2e/.demo/demo.webm`, and the Makefile turns that
// into the animated WebP that the README shows. Nothing passes or fails
// here: it is a camera, not a test.

import { chromium, type Locator } from '@playwright/test';
import { mkdirSync, readdirSync, renameSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import config from '../playwright.config.ts';
import { DEBRIEF_1, PAGE_1, PAGE_STEPS, QUIZ_1, SHEET, type PlannedPart } from './agent.ts';
import { start } from './server.ts';

const OUT = join(import.meta.dirname, '.demo');
const SIZE = { width: 1280, height: 800 };

const SYLLABUS: PlannedPart[] = [
  {
    title: 'The timer',
    pages: [
      { id: '1', kind: 'lesson', title: 'The election timer' },
      { id: '2', kind: 'lesson', title: 'Two candidates' },
    ],
  },
  {
    title: 'The vote',
    pages: [
      { id: '3', kind: 'lesson', title: 'One vote per term' },
      { id: 'c1', kind: 'checkpoint', title: 'Checkpoint' },
    ],
  },
];

const QUESTION = 'Why would the votes split again and again?';
const ANSWER =
  'With one fixed timeout, every follower wakes up in the same millisecond. Each one ' +
  'votes for **itself**, so no one gets a majority, and the next term starts the same way.';
const OPEN_ANSWER = 'So that two followers rarely time out at once.';

mkdirSync(OUT, { recursive: true });
// The video of an earlier run would be taken for this one at the end.
for (const name of readdirSync(OUT).filter((name) => name.endsWith('.webm'))) {
  unlinkSync(join(OUT, name));
}

const server = await start();
const { agent } = server;

await agent.course('Raft elections');
await agent.syllabus(SYLLABUS);
await agent.page('1', PAGE_1);
let waiting = agent.wait(0, 300);

const browser = await chromium.launch(config.use?.launchOptions);
const context = await browser.newContext({
  viewport: SIZE,
  colorScheme: 'dark',
  recordVideo: { dir: OUT, size: SIZE },
});
const page = await context.newPage();
// The video starts with the page, and the page starts empty.
const filmed = Date.now();

/** Let the picture rest, so that an eye can follow what just happened. */
const beat = (ms = 900) => page.waitForTimeout(ms);

/** Scroll a block into view the way a reader does, not in one jump. */
async function show(block: Locator, where: ScrollLogicalPosition = 'center') {
  await block.evaluate((el, at) => el.scrollIntoView({ behavior: 'smooth', block: at }), where);
  await beat(1100);
}

/** Select a phrase of the lesson, as the learner does before a question. */
async function select(phrase: string) {
  await page.evaluate((wanted) => {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const at = node.textContent?.indexOf(wanted) ?? -1;
      if (at >= 0) {
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + wanted.length);
        getSelection()?.removeAllRanges();
        getSelection()?.addRange(range);
        return;
      }
    }
    throw new Error(`no text ${wanted}`);
  }, phrase);
}

try {
  await page.goto(server.url);
  await page.locator('.agent-pill', { hasText: 'Agent connected' }).waitFor();
  await page.getByRole('heading', { name: 'The election timer', level: 1 }).waitFor();
  // The empty page is cut: the README plays the recording in a loop, and
  // the loop starts here.
  writeFileSync(join(OUT, 'start'), `${(Date.now() - filmed) / 1000}\n`);
  // The syllabus, and the first page with an agent that listens.
  await beat(2200);

  // A question on a phrase of the lesson, and the answer of the agent in
  // the thread.
  const anchor = page.locator('.thread-anchor', {
    has: page.getByRole('heading', { name: 'Why a random part' }),
  });
  await show(anchor, 'start');
  await select('the votes would split');
  await anchor.hover();
  await beat(500);
  await anchor.getByRole('button', { name: 'Ask' }).click();
  await anchor.getByRole('textbox').pressSequentially(QUESTION, { delay: 35 });
  await beat(500);
  await anchor.getByRole('button', { name: 'Send' }).click();
  const asked = await waiting;
  await beat(1500);
  await agent.reply('1-1', ANSWER);
  await anchor.locator('.thread-comment[data-author="agent"]').waitFor();
  await page.mouse.move(0, 0);
  await beat(2800);

  // The code, read from git: a marked line, and a hidden one to complete.
  await show(page.locator('.excerpt'));
  await beat(1800);

  // A quiz under the lesson: an open question, then a choice graded at once.
  waiting = agent.wait(asked.next, 300);
  await agent.quiz('1', QUIZ_1);
  const quiz = page.locator('.quiz[data-quiz="1"]');
  await quiz.waitFor();
  await show(quiz);
  await quiz.getByRole('textbox').pressSequentially(OPEN_ANSWER, { delay: 35 });
  await beat(500);
  await quiz.getByRole('button', { name: 'Send' }).click();
  await quiz.locator('.quiz-choices').waitFor();
  await show(quiz.locator('.quiz-choices'));
  await quiz.getByRole('radio').nth(1).check();
  await beat(700);
  await quiz.getByRole('button', { name: 'Send' }).click();
  await quiz.locator('.quiz-debrief').waitFor();
  const answered = await waiting;
  await beat(2000);

  // The debrief of the agent: the grade of the open answer, the code that
  // explains the rest, and a diagram.
  await agent.debrief('1', 1, DEBRIEF_1, { 'random-timeout': 'partial' });
  const debrief = quiz.locator('.debrief');
  await debrief.locator('.diagram-svg svg').waitFor();
  await show(debrief, 'start');
  await beat(2800);

  // Next: the agent writes the page while the learner waits.
  waiting = agent.wait(answered.next, 300);
  await show(page.getByRole('button', { name: 'Next' }));
  await page.getByRole('button', { name: 'Next' }).click();
  const next = await waiting;
  await agent.status('Writing page 2: two candidates');
  const end = page.locator('.page-end', { hasText: 'Writing page 2' });
  await end.waitFor();
  await show(end);
  await beat(1800);
  await agent.page('2', PAGE_STEPS);
  waiting = agent.wait(next.next, 300);
  await page.locator('.ready').waitFor();
  await beat(1400);
  await page.getByRole('button', { name: 'Next' }).click();
  await page.getByRole('heading', { name: 'Two candidates' }).waitFor();
  await beat(1200);

  // Steps: the learner plays them one by one, and the cells that changed
  // stand out.
  const steps = page.locator('figure.steps');
  await show(steps);
  for (let i = 0; i < 3; i++) {
    await steps.getByRole('button', { name: 'Next step', exact: true }).click();
    await beat(1800);
  }

  // The end of the course: the cheatsheet, to print or to export.
  await agent.cheatsheet(SHEET);
  await agent.finish();
  const note = page.locator('.sheet-ready');
  await note.waitFor();
  await beat(1400);
  await note.getByRole('link', { name: 'Open it' }).click();
  await page.locator('.cheatsheet .diagram-svg svg').first().waitFor();
  await beat(3000);
} finally {
  await context.close();
  await browser.close();
  await server.stop();
}

// Playwright names a video after the page. One recording, one name.
const [made] = readdirSync(OUT).filter((name) => name.endsWith('.webm'));
renameSync(join(OUT, made), join(OUT, 'demo.webm'));
console.log(`the recording is in ${join(OUT, 'demo.webm')}`);
