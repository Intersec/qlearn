// The course on the screen, and what the agent writes to it.
//
// The page learns of each write of the agent through the events route
// (section 8.3 of the design). It reads again only what a write touches.
// The place of the learner rides in the fragment of the address (section
// 8.1), so this store writes the history of the browser too.

import { defineStore } from 'pinia';
import { computed, ref, toRaw } from 'vue';
import { ApiError, api, serverGone } from '@/api/client';
import type { Course, Event, EventBatch, Page, PageRef, QuizAnswer } from '@/api/types';
import { format, parse } from '@/place';

/** The agent, as the top bar names it. The states of the terminal line. */
export type Presence = 'absent' | 'connected' | 'working' | 'left';

/**
 * What the end of the page offers. `next` asks for the next page, or opens
 * it when the agent wrote it already. `asked` waits for an agent that has
 * not taken the request yet. `preparing` says that the agent writes the
 * next page.
 */
export type Ending = 'next' | 'asked' | 'preparing';

/** How a move of the learner goes into the history of the browser. */
type Move = 'push' | 'replace';

/** How a move ends. A newer move replaced a `superseded` one. */
type Shown = 'shown' | 'failed' | 'superseded';

export const useCourse = defineStore('course', () => {
  const course = ref<Course | null>(null);
  /** The version of the server, as `qlearn --version` says. */
  const version = ref('');
  /** The page that the agent pushed last. */
  const current = ref<string | null>(null);
  /** The page on the screen. */
  const page = ref<Page | null>(null);
  /** A page that the agent pushed while the learner read another one. */
  const ready = ref<string | null>(null);
  /**
   * The Next request that the agent has not answered yet. `taken` once the
   * agent prepares a page since the request: a wait handed it over.
   */
  const asked = ref<{ page: string; taken: boolean } | null>(null);

  const agent = ref(false);
  const working = ref(false);
  /** The agent works on a Next request, where `working` may be a fix. */
  const preparing = ref(false);
  const left = ref(false);
  const status = ref<string | null>(null);
  /** The server stopped. A new server has a new token, so the page is dead. */
  const stopped = ref(false);
  const error = ref<string | null>(null);

  const presence = computed<Presence>(() => {
    if (working.value) {
      return 'working';
    }
    if (agent.value) {
      return 'connected';
    }
    return left.value ? 'left' : 'absent';
  });

  /** The pages of the syllabus, in the order of the course. */
  const pages = computed<PageRef[]>(() => course.value?.parts.flatMap((part) => part.pages) ?? []);

  function titleOf(id: string): string | undefined {
    return pages.value.find((entry) => entry.id === id)?.title;
  }

  /**
   * The page after the one on the screen, when the agent wrote it. Next
   * then opens it: a request would ask the agent for a page that exists.
   */
  const following = computed<string | null>(() => {
    const at = pages.value.findIndex((entry) => entry.id === page.value?.id);
    if (at < 0) {
      return null;
    }
    return pages.value.slice(at + 1).find((entry) => entry.state !== 'planned')?.id ?? null;
  });

  const ending = computed<Ending>(() => {
    if (following.value) {
      return 'next';
    }
    if (preparing.value) {
      return 'preparing';
    }
    return asked.value && asked.value.page === page.value?.id ? 'asked' : 'next';
  });

  /**
   * Only the newest move shows its page. Two quick clicks are two moves
   * that overlap, and the older one must not land over the newer one.
   */
  let moves = 0;

  /**
   * How many times the agent pushed each page since this tab opened. A
   * broken block is reported once per version of its page.
   */
  const pushes = new Map<string, number>();
  const reported = new Set<string>();

  /**
   * Tell the agent that the page cannot draw a block. A page draws a
   * diagram again on a theme change or a visit, and the agent needs to
   * hear of it once per version.
   */
  async function broken(id: string, block: string, error: string) {
    const key = `${id} ${pushes.get(id) ?? 0} ${block}`;
    if (reported.has(key)) {
      return;
    }
    reported.add(key);
    try {
      await api.broken(id, block, error);
    } catch (e) {
      // A newer push took the block away: the agent has nothing to fix.
      if (e instanceof ApiError && e.code === 'notFound') {
        return;
      }
      reported.delete(key);
      fail(e);
    }
  }

  /**
   * Send a part of the answer to the quiz `n` of a page. Answers whether
   * the server took it, so the quiz keeps what the learner typed when not.
   *
   * The server answers the page with the part in place, and the right
   * options once the choices are sent. A quiz answered in full may answer
   * the page, so the syllabus reads its state again.
   */
  async function answer(id: string, n: number, sent: QuizAnswer): Promise<boolean> {
    const at = moves;
    let read: Page;
    try {
      read =
        'open' in sent ? await api.open(id, n, sent.open) : await api.choices(id, n, sent.choices);
    } catch (e) {
      fail(e);
      // Another tab sent this part, and only the last part tells the
      // others: the quiz moves to the step that the server holds.
      if (e instanceof ApiError && e.code === 'conflict') {
        await refresh(id);
      }
      return false;
    }
    if (at === moves && page.value?.id === id) {
      page.value = read;
      error.value = null;
    }
    const quiz = read.quizzes[n - 1];
    const done =
      quiz !== undefined &&
      (quiz.open === null || quiz.open.sent !== null) &&
      quiz.choices.every((choice) => choice.graded !== null);
    if (done) {
      try {
        await readSession();
      } catch (e) {
        fail(e);
      }
    }
    return true;
  }

  /** Show a page. `none` leaves the address alone: the browser holds it. */
  async function show(id: string, move: Move | 'none'): Promise<Shown> {
    const mine = (moves += 1);
    let read: Page;
    try {
      read = await api.page(id);
    } catch (e) {
      if (mine !== moves) {
        return 'superseded';
      }
      fail(e);
      return 'failed';
    }
    if (mine !== moves) {
      return 'superseded';
    }

    page.value = read;
    error.value = null;
    if (ready.value === id) {
      ready.value = null;
    }
    if (move !== 'none') {
      write(id, move);
    }
    return 'shown';
  }

  /**
   * Read the page on the screen again. Not a move: it must not cancel a
   * move of the learner, and lands only if no move came since.
   */
  async function refresh(id: string) {
    const at = moves;
    let read: Page;
    try {
      read = await api.page(id);
    } catch (e) {
      if (at === moves && page.value?.id === id) {
        fail(e);
      }
      return;
    }
    if (at === moves && page.value?.id === id) {
      page.value = read;
    }
  }

  function write(id: string, move: Move) {
    const fragment = format({ page: id });
    if (location.hash === fragment) {
      return;
    }

    // The first place of a visit takes the address that the browser
    // opened, so Back from it leaves the course, not lands on a bare one.
    if (move === 'push' && parse(location.hash) !== null) {
      history.pushState(null, '', fragment);
    } else {
      history.replaceState(null, '', fragment);
    }
  }

  function fail(e: unknown) {
    // The loop of the events finds out that the server is gone, and says
    // it once.
    if (!serverGone(e)) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  }

  /**
   * Open the page that the address names, or the current page when it
   * names none. A page with no content leaves the current page in its
   * place, with the reason.
   */
  async function openAddress(move: Move | 'none') {
    const named = parse(location.hash)?.page;
    const id = named ?? current.value;
    if (!id || (await show(id, move)) !== 'failed') {
      return;
    }

    const reason = error.value;
    if (named && current.value && named !== current.value) {
      if ((await show(current.value, 'replace')) === 'shown') {
        error.value = reason;
      }
    }
  }

  /**
   * Read the course, and the page on the screen, again.
   *
   * The events in the gap are lost, so a page that the agent pushed in it
   * is found by the current page of the session.
   */
  async function reload() {
    const before = current.value;
    await readSession();
    const shown = page.value?.id;
    if (!shown) {
      await openAddress('replace');
      return;
    }
    if (current.value && current.value !== before) {
      asked.value = null;
      if (current.value !== shown) {
        ready.value = current.value;
      }
    }
    await refresh(shown);
  }

  async function readSession() {
    const session = await api.session();
    course.value = session.course;
    current.value = session.current;
    version.value = session.version;
  }

  /** The learner opens a page: a link, or Next on a page that exists. */
  async function open(id: string) {
    await show(id, 'push');
  }

  /** Back or forward: the browser already holds the address. */
  async function follow() {
    await openAddress('none');
  }

  /** Next: open the following page, or ask the agent for one. */
  async function next() {
    const shown = page.value;
    if (!shown) {
      return;
    }
    if (following.value) {
      await open(following.value);
      return;
    }
    if (asked.value?.page === shown.id) {
      return;
    }

    // Asked before the answer, so a second press sends no second request.
    const request = { page: shown.id, taken: false };
    asked.value = request;
    try {
      await api.next(shown.id);
    } catch (e) {
      // The ref holds a proxy of the request.
      if (asked.value && toRaw(asked.value) === request) {
        asked.value = null;
      }
      fail(e);
    }
  }

  function hear(batch: EventBatch) {
    // The page cannot see a wait hand the request over, but the agent
    // prepares a page from then until its next wait. An idle event is no
    // sign: the wait that takes the request pushes one as it opens.
    if (asked.value) {
      if (batch.preparing && !preparing.value) {
        asked.value.taken = true;
      } else if (!batch.preparing && asked.value.taken) {
        asked.value = null;
      }
    }
    agent.value = batch.agent;
    working.value = batch.working;
    preparing.value = batch.preparing;
    left.value = batch.left;
    status.value = batch.status;
  }

  /**
   * Read again what the writes of the agent touch.
   *
   * A new page never takes the learner away from the one on the screen
   * (section 3): it is ready, and the learner opens it. Only a page that
   * shows nothing yet opens it.
   */
  async function settle(events: Event[]) {
    let session = false;
    let again = false;
    let first: string | null = null;
    let fresh: string | null = null;

    for (const event of events) {
      // An answer in another tab may answer its page, and shows its
      // grades there.
      if (event.kind === 'answer' && event.page) {
        session = true;
        again ||= event.page === page.value?.id;
        continue;
      }
      if (event.author !== 'agent') {
        continue;
      }
      if (event.kind === 'course' || event.kind === 'syllabus') {
        session = true;
      }
      // A quiz may take its page back to taught, and shows under its lesson.
      // It is no new page: it moves nothing and makes nothing ready.
      if (event.kind === 'quiz' && event.page) {
        session = true;
        again ||= event.page === page.value?.id;
        continue;
      }
      // A debrief shows under its quiz, and may change the state and the
      // score of its page. A new version of it may hold the same broken
      // diagram, which the agent must hear of again.
      if (event.kind === 'debrief' && event.page) {
        session = true;
        pushes.set(event.page, (pushes.get(event.page) ?? 0) + 1);
        again ||= event.page === page.value?.id;
        continue;
      }
      if (event.kind !== 'page' || !event.page) {
        continue;
      }

      // A pushed page joins the syllabus, with its title.
      session = true;
      pushes.set(event.page, (pushes.get(event.page) ?? 0) + 1);
      // A push of the page that the learner left fixes it, and answers no
      // Next.
      if (asked.value?.page !== event.page) {
        asked.value = null;
      }
      if (event.page === page.value?.id) {
        again = true;
      } else if (!page.value) {
        first = event.page;
      } else {
        fresh = event.page;
      }
    }

    // The note of a ready page names it by its title, which the course holds.
    if (session) {
      await readSession();
    }
    if (fresh) {
      ready.value = fresh;
    }
    if (first) {
      await show(first, 'replace');
    } else if (again && page.value) {
      await refresh(page.value.id);
    }
  }

  let listening = false;

  /**
   * Read the course, then listen to the writes of the agent until the
   * server stops.
   *
   * The number to start from is taken before the course is read, so a
   * write between the two is heard. A failure waits a little and starts
   * over, and the course is read again, since a write may have been missed.
   */
  async function listen() {
    if (listening) {
      return;
    }
    listening = true;
    let stale = true;
    for (;;) {
      try {
        const start = await api.events();
        hear(start);
        if (stale) {
          await reload();
          stale = false;
        }
        let after = start.next;
        for (;;) {
          const batch = await api.events(after);
          after = batch.next;
          hear(batch);
          if (batch.reset) {
            stale = true;
            break;
          }
          await settle(batch.events);
        }
      } catch (e) {
        stale = true;
        if (serverGone(e) && (await stillGone())) {
          stopped.value = true;
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 5000));
      }
    }
  }

  /** Ask once more, a second later, before the page says the server is gone. */
  async function stillGone(): Promise<boolean> {
    await new Promise((resolve) => setTimeout(resolve, 1000));
    try {
      await api.events();
      return false;
    } catch (e) {
      return serverGone(e);
    }
  }

  return {
    course,
    version,
    current,
    page,
    ready,
    asked,
    agent,
    working,
    left,
    status,
    stopped,
    error,
    presence,
    following,
    ending,
    titleOf,
    open,
    follow,
    next,
    answer,
    broken,
    listen,
  };
});
