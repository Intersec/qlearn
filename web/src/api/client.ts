// Every call to the server. The token travels as a cookie that the first
// page load left behind, so nothing here carries it.

import type { ErrorBody, Event, EventBatch, Page, Pick, Session, Thread } from './types';

export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
  });

  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as ErrorBody | null;
    const error = body?.error;
    const code = response.status === 401 ? 'unauthorized' : 'failed';
    throw new ApiError(error?.code ?? code, error?.message ?? response.statusText);
  }
  return response.json() as Promise<T>;
}

/**
 * True when the server that served this page is gone for good.
 *
 * The server is on the loopback, so a request that reaches nothing means
 * that it stopped. A refused token means that another server took its
 * port, and the cookie of this page opens nothing there.
 */
export function serverGone(error: unknown): boolean {
  return error instanceof TypeError || (error instanceof ApiError && error.code === 'unauthorized');
}

export const api = {
  session: () => call<Session>('/api/session'),

  page: (id: string) => call<Page>(`/api/pages/${encodeURIComponent(id)}`),

  /** Ask the agent for the page after this one. Answers the request. */
  next: (id: string) =>
    call<Event>(`/api/pages/${encodeURIComponent(id)}/next`, { method: 'POST' }),

  /** Send the open answer of the quiz `n` of a page. Answers the page. */
  open: (id: string, n: number, text: string) =>
    call<Page>(`${quiz(id, n)}/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text }),
    }),

  /**
   * Send the choices of the quiz `n` of a page. The server grades them, and
   * answers the page with the right options.
   */
  choices: (id: string, n: number, choices: Pick[]) =>
    call<Page>(`${quiz(id, n)}/choices`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ choices }),
    }),

  /**
   * Report a block that the page cannot draw. The agent gets its source
   * and the error, and pushes the page again. Answers the request.
   */
  broken: (id: string, block: string, error: string) =>
    call<Event>(`/api/pages/${encodeURIComponent(id)}/broken`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ block, error }),
    }),

  /**
   * Ask about a block of a page, with the text that the learner selected
   * in it. The agent gets the question. Answers the thread.
   */
  ask: (id: string, block: string, quote: string | null, body: string) =>
    call<Thread>(`/api/pages/${encodeURIComponent(id)}/threads`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ block, quote, body }),
    }),

  /** Write in a thread. The agent gets it as a question. Answers the thread. */
  reply: (thread: string, body: string) =>
    call<Thread>(`/api/threads/${encodeURIComponent(thread)}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ body }),
    }),

  /**
   * The writes to the store after a number, held until one comes. A wait
   * names the page, so the server knows whether a page is open. With no
   * number, the server answers at once with the number to start from.
   */
  events: (after?: number) =>
    call<EventBatch>(
      `/api/events${query({ after: num(after), who: after === undefined ? undefined : 'page' })}`,
    ),
};

/** The address of the quiz `n` of a page, from 1. */
function quiz(id: string, n: number): string {
  return `/api/pages/${encodeURIComponent(id)}/quizzes/${n}`;
}

function num(value: number | undefined): string | undefined {
  return value === undefined ? undefined : String(value);
}

function query(params: Record<string, string | undefined>): string {
  const pairs = Object.entries(params).filter(([, value]) => value !== undefined);
  if (pairs.length === 0) {
    return '';
  }
  return `?${new URLSearchParams(pairs as [string, string][]).toString()}`;
}
