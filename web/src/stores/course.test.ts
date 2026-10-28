// @vitest-environment jsdom
// The store writes the place in the address, so it needs a history.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { ApiError, api } from '@/api/client';
import type { Course, Event, EventBatch, Page, PageRef } from '@/api/types';
import { useCourse } from './course';

/** The HTML of the first block, which these pages hold as text. */
function firstHtml(page: Page | null | undefined): string | undefined {
  const block = page?.blocks[0];
  return block?.kind === 'text' ? block.html : undefined;
}

function pageRef(id: string, state: PageRef['state'] = 'taught'): PageRef {
  return { id, kind: 'lesson', title: `Page ${id}`, state, score: null };
}

function courseOf(pages: PageRef[]): Course {
  return {
    version: 1,
    slug: 'raft',
    title: 'Raft, the leader election',
    goal: '',
    profile: '',
    quizMode: 'mixed',
    status: 'running',
    followUp: null,
    parts: [{ title: '', pages }],
    findings: [],
    created: '2026-01-01T00:00:00Z',
    updated: '2026-01-01T00:00:00Z',
  };
}

function pageOf(id: string, html = `<h1>Page ${id}</h1>`): Page {
  return { id, blocks: [{ id: `block${id}`, kind: 'text', html }] };
}

function batch(fields: Partial<EventBatch> = {}): EventBatch {
  return {
    events: [],
    next: 0,
    reset: false,
    agent: false,
    working: false,
    preparing: false,
    left: false,
    status: null,
    ...fields,
  };
}

function idle(seq: number): Event {
  return { seq, kind: 'idle', author: 'agent', page: null, id: null };
}

function pushed(seq: number, page: string): Event {
  return { seq, kind: 'page', author: 'agent', page, id: null };
}

/**
 * A server in memory. A wait on the events gets the next batch that the
 * test sends, and holds until there is one, as the real route does.
 */
class FakeServer {
  pages = new Map<string, Page>();
  course: Course = courseOf([]);
  current: string | null = null;
  /** The `after` of each wait, `undefined` for the one that starts. */
  waits: (number | undefined)[] = [];
  /** The pages that a Next was sent from. */
  nexts: string[] = [];
  /** Each block that the page reported broken: its page, its id, the error. */
  broken: [string, string, string][] = [];
  /** The error that the next report gets, if any. */
  refuse: ApiError | null = null;
  reads = { session: 0, page: [] as string[] };
  /** Reads of a page that wait until the test lets them answer. */
  private gates = new Map<string, Promise<void>>();
  private queued: EventBatch[] = [];
  private held: ((batch: EventBatch) => void) | null = null;

  constructor() {
    vi.spyOn(api, 'session').mockImplementation(async () => {
      this.reads.session += 1;
      // A copy, as a fetch makes: Vue sees no change in the same object.
      const course = structuredClone(this.course);
      return { course, current: this.current, version: '0.1.0 (test)' };
    });
    vi.spyOn(api, 'page').mockImplementation(async (id) => {
      this.reads.page.push(id);
      await this.gates.get(id);
      const page = this.pages.get(id);
      if (!page) {
        throw new ApiError('notFound', `page ${id} has no content yet`);
      }
      return structuredClone(page);
    });
    vi.spyOn(api, 'next').mockImplementation(async (id) => {
      this.nexts.push(id);
      return { seq: 40 + this.nexts.length, kind: 'next', author: 'learner', page: id, id: null };
    });
    vi.spyOn(api, 'broken').mockImplementation(async (id, block, error) => {
      const refused = this.refuse;
      this.refuse = null;
      if (refused) {
        throw refused;
      }
      this.broken.push([id, block, error]);
      return {
        seq: 50 + this.broken.length,
        kind: 'broken',
        author: 'learner',
        page: id,
        id: null,
      };
    });
    vi.spyOn(api, 'events').mockImplementation((after) => {
      this.waits.push(after);
      // The first wait answers at once with the number to start from.
      if (after === undefined) {
        return Promise.resolve(batch({ next: 10 }));
      }
      const ready = this.queued.shift();
      if (ready) {
        return Promise.resolve(ready);
      }
      return new Promise((resolve) => {
        this.held = resolve;
      });
    });
  }

  /** Push a page, as `qlearn page` does. Answers its event. */
  push(id: string, seq: number, html?: string): Event {
    this.pages.set(id, pageOf(id, html));
    if (!this.course.parts[0].pages.some((ref) => ref.id === id)) {
      this.course.parts[0].pages.push(pageRef(id));
    }
    this.current = id;
    return pushed(seq, id);
  }

  /** Hold the reads of a page. Answers what lets them go. */
  hold(id: string): () => void {
    let release = () => {};
    this.gates.set(
      id,
      new Promise((resolve) => {
        release = () => {
          this.gates.delete(id);
          resolve();
        };
      }),
    );
    return release;
  }

  send(next: EventBatch) {
    const held = this.held;
    if (held) {
      this.held = null;
      held(next);
    } else {
      this.queued.push(next);
    }
  }
}

let server: FakeServer;

beforeEach(() => {
  setActivePinia(createPinia());
  history.replaceState(null, '', '/');
  server = new FakeServer();
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** Start the store and wait until it holds a wait open. */
async function started(): Promise<ReturnType<typeof useCourse>> {
  const store = useCourse();
  void store.listen();
  await vi.waitFor(() => expect(server.waits).toContain(10));
  return store;
}

describe('the place', () => {
  it('opens the current page when the address names none', async () => {
    server.push('1', 1);
    server.push('2', 2);
    const push = vi.spyOn(history, 'pushState');

    const store = await started();

    expect(store.page?.id).toBe('2');
    expect(location.hash).toBe('#page=2');
    // Back from the first page leaves the course.
    expect(push).not.toHaveBeenCalled();
  });

  it('opens the page that the address names, as a reload does', async () => {
    server.push('1', 1);
    server.push('2', 2);
    history.replaceState(null, '', '/#page=1');

    const store = await started();

    expect(store.page?.id).toBe('1');
    expect(location.hash).toBe('#page=1');
  });

  it('opens the current page in place of one with no content, with the reason', async () => {
    server.push('1', 1);
    history.replaceState(null, '', '/#page=9');

    const store = await started();

    expect(store.page?.id).toBe('1');
    expect(location.hash).toBe('#page=1');
    expect(store.error).toBe('page 9 has no content yet');
  });

  it('shows nothing before the first page', async () => {
    const store = await started();

    expect(store.page).toBeNull();
    expect(location.hash).toBe('');
  });

  it('walks the pages read with back and forward', async () => {
    server.push('1', 1);
    server.push('2', 2);
    history.replaceState(null, '', '/#page=1');
    const store = await started();
    window.addEventListener('popstate', () => void store.follow(), { once: true });

    await store.open('2');
    expect(location.hash).toBe('#page=2');

    history.back();
    await vi.waitFor(() => expect(store.page?.id).toBe('1'));
    expect(location.hash).toBe('#page=1');
  });

  it('keeps a newer move when an older one ends', async () => {
    server.push('1', 1);
    server.push('3', 2);
    server.push('2', 3);
    history.replaceState(null, '', '/#page=3');
    const store = await started();

    const release = server.hold('1');
    history.pushState(null, '', '#page=1');
    const back = store.follow();
    await store.open('3');
    release();
    await back;

    // The older move lost to the newer one: it did not fail.
    expect(store.page?.id).toBe('3');
    expect(location.hash).toBe('#page=3');
    expect(store.error).toBeNull();
  });

  it('keeps a move when the page on the screen is read again', async () => {
    server.push('1', 1);
    server.push('2', 2);
    history.replaceState(null, '', '/#page=1');
    const store = await started();

    const release = server.hold('2');
    const opening = store.open('2');
    server.send(batch({ next: 11, events: [server.push('1', 11, '<p>fixed</p>')] }));
    await vi.waitFor(() => expect(firstHtml(store.page)).toBe('<p>fixed</p>'));
    release();
    await opening;

    expect(store.page?.id).toBe('2');
    expect(location.hash).toBe('#page=2');
  });
});

describe('the events', () => {
  it('waits after the number of the first call', async () => {
    await started();

    expect(server.waits).toEqual([undefined, 10]);
    expect(api.events).toHaveBeenLastCalledWith(10);
  });

  it('reads the course and the page again on a reset', async () => {
    server.push('1', 1);
    const store = await started();
    server.pages.set('1', pageOf('1', '<p>written again</p>'));
    server.course = { ...server.course, title: 'Raft, the log' };
    const sessions = server.reads.session;

    server.send(batch({ reset: true, next: 0 }));

    await vi.waitFor(() => expect(store.course?.title).toBe('Raft, the log'));
    expect(server.reads.session).toBe(sessions + 1);
    expect(firstHtml(store.page)).toBe('<p>written again</p>');
    // A new start: the wait after a reset takes the number again.
    expect(server.waits).toEqual([undefined, 10, undefined, 10]);
  });

  it('says that a page pushed before a reset is ready', async () => {
    server.push('1', 1);
    const store = await started();
    await store.next();

    // The agent answers, and the page misses the event.
    server.push('2', 11);
    server.send(batch({ reset: true, next: 0 }));

    await vi.waitFor(() => expect(store.current).toBe('2'));
    await vi.waitFor(() => expect(store.ready).toBe('2'));
    expect(store.page?.id).toBe('1');
    expect(store.asked).toBeNull();
  });

  it('names the presence of the agent and its status', async () => {
    const store = await started();
    expect(store.presence).toBe('absent');

    server.send(batch({ next: 11, agent: true }));
    await vi.waitFor(() => expect(store.presence).toBe('connected'));

    server.send(batch({ next: 12, agent: true, working: true, status: 'Writing page 2' }));
    await vi.waitFor(() => expect(store.presence).toBe('working'));
    expect(store.status).toBe('Writing page 2');

    server.send(batch({ next: 13, left: true }));
    await vi.waitFor(() => expect(store.presence).toBe('left'));
    expect(store.status).toBeNull();
  });
});

describe('a page that the agent pushes', () => {
  it('is ready, and does not move the learner', async () => {
    server.push('1', 1);
    const store = await started();

    server.send(batch({ next: 11, events: [server.push('2', 11)] }));

    await vi.waitFor(() => expect(store.ready).toBe('2'));
    expect(store.page?.id).toBe('1');
    expect(location.hash).toBe('#page=1');
    expect(store.titleOf('2')).toBe('Page 2');
  });

  it('is no longer ready once the learner opens it', async () => {
    server.push('1', 1);
    const store = await started();
    server.send(batch({ next: 11, events: [server.push('2', 11)] }));
    await vi.waitFor(() => expect(store.ready).toBe('2'));

    await store.open('2');

    expect(store.ready).toBeNull();
    expect(location.hash).toBe('#page=2');
  });

  it('is read again when it is the page on the screen', async () => {
    server.push('1', 1);
    const store = await started();

    server.send(batch({ next: 11, events: [server.push('1', 11, '<p>fixed</p>')] }));

    await vi.waitFor(() => expect(firstHtml(store.page)).toBe('<p>fixed</p>'));
    expect(store.ready).toBeNull();
  });

  it('opens when the page showed nothing yet', async () => {
    const store = await started();

    server.send(batch({ next: 11, events: [server.push('1', 11)] }));

    await vi.waitFor(() => expect(store.page?.id).toBe('1'));
    expect(location.hash).toBe('#page=1');
    expect(store.ready).toBeNull();
  });

  it('is not ready when the learner pushed it', async () => {
    server.push('1', 1);
    const store = await started();
    server.pages.set('2', pageOf('2'));

    const event: Event = { seq: 11, kind: 'page', author: 'learner', page: '2', id: null };
    server.send(batch({ next: 11, events: [event] }));
    server.send(batch({ next: 12, agent: true }));

    await vi.waitFor(() => expect(store.presence).toBe('connected'));
    expect(store.ready).toBeNull();
  });
});

describe('next', () => {
  it('asks the agent on the last page, then says that the agent prepares one', async () => {
    server.push('1', 1);
    const store = await started();

    await store.next();

    expect(server.nexts).toEqual(['1']);
    expect(store.ending).toBe('asked');
    // Asked once: a second press sends nothing.
    await store.next();
    expect(server.nexts).toEqual(['1']);

    server.send(
      batch({ next: 42, agent: true, working: true, preparing: true, status: 'Writing page 2' }),
    );
    await vi.waitFor(() => expect(store.ending).toBe('preparing'));

    server.send(
      batch({
        next: 43,
        agent: true,
        working: true,
        preparing: true,
        events: [server.push('2', 43)],
      }),
    );
    await vi.waitFor(() => expect(store.ready).toBe('2'));
    expect(store.page?.id).toBe('1');
    expect(store.asked).toBeNull();
    // The page exists now, so Next opens it.
    expect(store.ending).toBe('next');

    await store.next();
    expect(store.page?.id).toBe('2');
    expect(server.nexts).toEqual(['1']);
  });

  it('opens the following page when the agent wrote it already', async () => {
    server.push('1', 1);
    server.push('2', 2);
    history.replaceState(null, '', '/#page=1');
    const store = await started();

    await store.next();

    expect(server.nexts).toEqual([]);
    expect(store.page?.id).toBe('2');
    expect(location.hash).toBe('#page=2');
  });

  it('skips a page of the syllabus that has no content', async () => {
    server.push('1', 1);
    server.course.parts[0].pages.push(pageRef('2', 'planned'));
    const store = await started();

    await store.next();

    expect(server.nexts).toEqual(['1']);
  });

  it('sends one request for a double press', async () => {
    server.push('1', 1);
    const store = await started();

    await Promise.all([store.next(), store.next()]);

    expect(server.nexts).toEqual(['1']);
    expect(store.ending).toBe('asked');
  });

  it('can be sent again when the request failed', async () => {
    server.push('1', 1);
    const store = await started();
    vi.mocked(api.next).mockRejectedValueOnce(new ApiError('internal', 'the disk is full'));

    await store.next();

    expect(store.asked).toBeNull();
    expect(store.error).toBe('the disk is full');
    expect(store.ending).toBe('next');
  });

  it('is answered when the agent waits again with no page', async () => {
    server.push('1', 1);
    const store = await started();
    await store.next();

    server.send(batch({ next: 41, agent: true, working: true, preparing: true }));
    await vi.waitFor(() => expect(store.ending).toBe('preparing'));
    // The agent ends the course: it writes, so its wait says it is idle.
    server.send(batch({ next: 43, agent: true, events: [idle(43)] }));

    await vi.waitFor(() => expect(store.asked).toBeNull());
    expect(store.ending).toBe('next');
  });

  it('is answered when the agent waits again with no write', async () => {
    server.push('1', 1);
    const store = await started();
    await store.next();

    server.send(batch({ next: 41, agent: true, working: true, preparing: true }));
    await vi.waitFor(() => expect(store.ending).toBe('preparing'));
    // No write: the wait pushes no idle, and only the work ends.
    server.send(batch({ next: 41, agent: true }));

    await vi.waitFor(() => expect(store.asked).toBeNull());
    expect(store.ending).toBe('next');
  });

  it('is not answered by the idle of the wait that takes it', async () => {
    server.push('1', 1);
    const store = await started();
    await store.next();

    // The wait opens after a write of the agent: it says that the agent is
    // idle, then hands the request over.
    server.send(batch({ next: 42, agent: true, events: [idle(42)] }));
    server.send(batch({ next: 42, agent: true, status: 'Reading the request' }));
    await vi.waitFor(() => expect(store.status).toBe('Reading the request'));

    expect(store.ending).toBe('asked');
  });
});

describe('a block that the page cannot draw', () => {
  it('is reported once per version of its page', async () => {
    server.push('1', 1);
    const store = await started();

    // A theme change or a visit draws the diagram again.
    await store.broken('1', 'd1', 'Parse error');
    await store.broken('1', 'd1', 'Parse error');
    await store.broken('1', 'd2', 'Parse error');
    expect(server.broken).toEqual([
      ['1', 'd1', 'Parse error'],
      ['1', 'd2', 'Parse error'],
    ]);

    // The agent pushes the page again, with the same diagram.
    server.send(batch({ next: 11, events: [server.push('1', 11)] }));
    await vi.waitFor(() => expect(server.reads.page).toEqual(['1', '1']));
    await store.broken('1', 'd1', 'Parse error');
    await store.broken('1', 'd1', 'Parse error');

    expect(server.broken).toHaveLength(3);
    expect(server.broken[2]).toEqual(['1', 'd1', 'Parse error']);
  });

  it('is reported again when the report failed', async () => {
    server.push('1', 1);
    const store = await started();
    server.refuse = new ApiError('failed', 'the disk is full');

    await store.broken('1', 'd1', 'Parse error');
    expect(store.error).toBe('the disk is full');
    await store.broken('1', 'd1', 'Parse error');

    expect(server.broken).toEqual([['1', 'd1', 'Parse error']]);
  });

  it('leaves Next to the learner while the agent fixes it', async () => {
    server.push('1', 1);
    const store = await started();
    await store.broken('1', 'd1', 'Parse error');

    // A wait hands the broken request over: the agent works, on this page.
    server.send(batch({ next: 51, agent: true, working: true }));
    await vi.waitFor(() => expect(store.presence).toBe('working'));
    expect(store.ending).toBe('next');

    await store.next();
    expect(server.nexts).toEqual(['1']);
    expect(store.ending).toBe('asked');

    // The agent pushes the fix and waits again: the wait takes the Next.
    server.send(batch({ next: 52, agent: true, events: [server.push('1', 52)] }));
    await vi.waitFor(() => expect(store.presence).toBe('connected'));
    expect(store.ending).toBe('asked');
    server.send(batch({ next: 52, agent: true, working: true, preparing: true }));
    await vi.waitFor(() => expect(store.ending).toBe('preparing'));
  });

  it('says nothing when a newer push took the block away', async () => {
    server.push('1', 1);
    const store = await started();
    server.refuse = new ApiError('notFound', 'page 1 holds no block "d1"');

    await store.broken('1', 'd1', 'Parse error');

    expect(store.error).toBeNull();
    expect(server.broken).toEqual([]);
  });
});
