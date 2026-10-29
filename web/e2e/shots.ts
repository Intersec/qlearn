// Screenshots of the interface, for a person or an agent to look at.
//
// `make shots` writes them under e2e/.shots. They are not a test: nothing
// passes or fails here, it only makes the interface visible.

import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import config from '../playwright.config.ts';
import {
  CHOICE_LEVEL,
  CHOICE_NEXT,
  DEBRIEF_1,
  PAGE_1,
  PAGE_2,
  PAGE_DIAGRAMS,
  PAGE_DIAGRAMS_FIXED,
  QUIZ_1,
  QUIZ_2,
  SYLLABUS,
} from './agent.ts';
import { start } from './server.ts';

const OUT = join(import.meta.dirname, '.shots');
mkdirSync(OUT, { recursive: true });

// The browser of the tests, set up as they set it up.
const browser = await chromium.launch(config.use?.launchOptions);

for (const scheme of ['light', 'dark'] as const) {
  // A server per theme: the loop moves the course forward, and each theme
  // walks it from the start.
  const server = await start();
  const { agent } = server;
  const page = await browser.newPage({
    viewport: { width: 1600, height: 1000 },
    colorScheme: scheme,
  });
  const complaints: string[] = [];
  page.on('response', (response) => {
    if (response.status() >= 400) {
      complaints.push(`${response.status()} ${response.url()}`);
    }
  });
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') {
      complaints.push(`${message.type()}: ${message.text()}`);
    }
  });

  try {
    await agent.course('Raft elections');
    await agent.syllabus(SYLLABUS);
    await agent.page('1', PAGE_1);
    const waited = agent.wait(0);

    // The first page, with an agent that listens.
    await page.goto(server.url);
    await page.locator('.agent-pill', { hasText: 'Agent connected' }).waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-page.png`) });

    // An excerpt of the repository, with a marked line and a hidden one.
    const excerpt = page.locator('.excerpt');
    await excerpt.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-excerpt.png`) });

    // The callouts, each with its marker.
    await page.locator('.callout').first().scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-callouts.png`) });

    // The quizzes under the lesson: the open question alone, then a quiz
    // of choices only.
    await agent.quiz('1', QUIZ_1);
    await agent.quiz('1', QUIZ_2);
    const quiz = page.locator('.quiz[data-quiz="2"]');
    await quiz.waitFor();
    await page.locator('.quiz[data-quiz="1"]').scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-quiz.png`) });
    await quiz.locator('input[type="radio"]').nth(1).check();
    await quiz.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-quiz-choices.png`) });

    // The learner answers the first quiz: the open answer, then the
    // choices with an Other box, then right or wrong at once.
    const first = page.locator('.quiz[data-quiz="1"]');
    await first.getByRole('textbox').fill('Two followers rarely time out at once.');
    await first.getByRole('button', { name: 'Send' }).click();
    await first.locator('.quiz-choices').waitFor();
    await first.getByRole('radio').first().check();
    await first.getByRole('textbox', { name: 'Other' }).fill('Unless the ids tie too');
    await first.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-quiz-step-2.png`) });
    await first.getByRole('button', { name: 'Send' }).click();
    await first.locator('.quiz-debrief').waitFor();
    const answered = await waited;
    await first.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-quiz-graded.png`) });

    // The second quiz answers the page, and the hidden line shows.
    const second = agent.wait(answered.next);
    await quiz.getByRole('radio').nth(3).check();
    await quiz.getByRole('button', { name: 'Send' }).click();
    await quiz.locator('.quiz-debrief').waitFor();
    const both = await second;
    await page.locator('.syllabus-page[data-state="answered"]').waitFor();
    await quiz.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-quiz-answered.png`) });
    await excerpt.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-excerpt-shown.png`) });

    // The agent debriefs both quizzes: each debrief shows under its quiz,
    // with the grade of the open answer, and the column shows the score
    // with its half.
    await agent.debrief('1', 1, DEBRIEF_1, { 'random-timeout': 'partial' });
    await agent.debrief(
      '1',
      2,
      '# Both right\n\nThe base sets the floor of the wait, and the random part its spread.\n',
    );
    await first.locator('.debrief .diagram-svg svg').waitFor();
    await page.locator('.syllabus-page[data-state="debriefed"]').waitFor();
    await first.locator('.debrief').scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-quiz-debriefed.png`) });

    // A question of the learner on a block, with the text that it selected
    // as the quote, then the reply of the agent under it, and a thread of
    // the agent about the whole page.
    const asked = agent.wait(both.next);
    const anchor = page.locator('.thread-anchor', {
      has: page.getByRole('heading', { name: 'Why a random part' }),
    });
    await anchor.scrollIntoViewIfNeeded();
    await page.evaluate(() => {
      const wanted = 'the votes would split';
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
    });
    await anchor.hover();
    await anchor.getByRole('button', { name: 'Ask' }).click();
    await anchor.getByRole('textbox').fill('Why would they split *again* and again?');
    await page.screenshot({ path: join(OUT, `${scheme}-ask.png`) });
    await anchor.getByRole('button', { name: 'Send' }).click();
    const question = await asked;
    await agent.reply(
      '1-1',
      'Each follower waits the **same** time, so all of them start an election in the same ' +
        'term, and each votes for itself.',
    );
    await anchor.locator('.thread-comment[data-author="agent"]').waitFor();
    await agent.comment('1', 'We stopped at the random part. Next time: `election_timeout`.');
    await page.locator('.threads-page .thread').waitFor();
    await page.mouse.move(0, 0);
    await anchor.locator('.thread').scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-thread.png`) });

    // The agent pushes the page again without that sentence: its thread
    // moves to the end of the page, with its quote.
    await agent.page('1', PAGE_1.replace('the votes would split', 'the votes split'));
    const earlier = page.locator('.threads-earlier');
    await earlier.waitFor();
    await earlier.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-thread-earlier.png`) });

    // Next, and the agent at work on the page after.
    const waitedNext = agent.wait(question.next);
    await page.locator('.agent-pill', { hasText: 'Agent connected' }).waitFor();
    await page.getByRole('button', { name: 'Next' }).click();
    const { next } = await waitedNext;
    await agent.status('Writing page 2: the vote');
    const end = page.locator('.page-end', { hasText: 'Writing page 2' });
    await end.waitFor();
    await end.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-preparing.png`) });

    // The page is ready, and the learner still reads the first one. The
    // agent waits again, as the procedure says, so the page shows it there.
    await agent.page('2', PAGE_2);
    void agent.wait(next);
    await page.locator('.ready').waitFor();
    await page.locator('.agent-pill', { hasText: 'Agent connected' }).waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-ready.png`) });

    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByRole('heading', { name: 'The vote' }).waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-page-2.png`) });

    // A diagram that Mermaid draws, and one that it refuses: the page shows
    // the error with the source, and sends it to the agent.
    await agent.page('3', PAGE_DIAGRAMS);
    await page.locator('.ready').getByRole('link', { name: 'Read it' }).click();
    await page.locator('.diagram-svg svg').waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-diagram.png`) });
    const broken = page.locator('.diagram-broken');
    await broken.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-diagram-broken.png`) });

    // The agent fixed it.
    await agent.page('3', PAGE_DIAGRAMS_FIXED);
    await broken.waitFor({ state: 'detached' });
    await page.locator('.diagram-svg svg').nth(1).scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-diagram-fixed.png`) });

    // The agent asks two choices: one at the top of the syllabus column,
    // and one at the end of the page. The learner answers the second one,
    // which then shows read only.
    await agent.choose(CHOICE_LEVEL);
    await agent.choose({ ...CHOICE_NEXT, page: '3' });
    const choice = page.locator('.lesson .choice');
    await page.locator('.syllabus .choice').waitFor();
    await choice.waitFor();
    await choice.getByRole('radio').first().check();
    await choice.getByRole('checkbox').nth(1).check();
    await choice.getByRole('textbox', { name: 'Other' }).nth(1).fill('and the restart');
    await choice.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(OUT, `${scheme}-choice.png`) });
    await choice.getByRole('button', { name: 'Send' }).click();
    await page.locator('.lesson .choice[data-answered="true"]').waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-choice-answered.png`) });

    // A narrow screen: the syllabus hides behind a button, and opens over
    // the page. The button says that a choice of the column waits.
    await page.setViewportSize({ width: 420, height: 860 });
    await page.screenshot({ path: join(OUT, `${scheme}-narrow.png`) });
    await page.getByRole('button', { name: 'Syllabus' }).click();
    await page.getByRole('navigation', { name: 'Syllabus' }).waitFor();
    await page.screenshot({ path: join(OUT, `${scheme}-narrow-syllabus.png`) });
  } finally {
    if (complaints.length) {
      console.log(`${scheme}: the page complained\n  ${complaints.join('\n  ')}`);
    }
    await page.close();
    await server.stop();
  }
}

await browser.close();
console.log(`screenshots in ${OUT}`);
