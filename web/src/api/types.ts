// The shapes that cross the wire, as `roadmap/design.md` names them.
//
// Written by hand. One insta snapshot per route pins the Rust side, in
// crates/qlearn/src/api/snapshots, so a struct that drifts from this file
// turns a Rust test red. Read the snapshot before you change a type here.

export type QuizMode = 'mixed' | 'current';
export type CourseStatus = 'new' | 'running' | 'finished';
export type PageKind = 'lesson' | 'checkpoint' | 'followup';
export type PageState = 'planned' | 'taught' | 'answered' | 'debriefed';

export interface Course {
  version: 1;
  slug: string;
  /** The subject, as the learner named it. */
  title: string;
  /** Markdown. */
  goal: string;
  /** Markdown: the background and the level of the learner. */
  profile: string;
  quizMode: QuizMode;
  status: CourseStatus;
  /** A date, YYYY-MM-DD. */
  followUp: string | null;
  parts: Part[];
  findings: Finding[];
  /** RFC 3339. */
  created: string;
  updated: string;
}

/** A course with no parts is one implicit part with no title. */
export interface Part {
  title: string;
  pages: PageRef[];
}

export interface PageRef {
  /** `1`, `7a`, `c1`, `f1`. */
  id: string;
  kind: PageKind;
  title: string;
  state: PageState;
  score: Score | null;
}

export interface Score {
  right: number;
  total: number;
}

export interface Finding {
  id: string;
  title: string;
  /** Markdown. */
  body: string;
  /** False reads "to check". */
  checked: boolean;
}

/** What `GET /api/session` answers. */
export interface Session {
  course: Course;
  /** The page that the agent pushed last, or null before the first push. */
  current: string | null;
  /** The release and the commit under it, as `qlearn --version` says. */
  version: string;
}

/**
 * A block of a page. The server writes the HTML from a short list of tags,
 * and the page still passes it through DOMPurify. The other kinds of
 * section 4.1 join this type with the step that teaches the server them.
 */
export interface TextBlock {
  id: string;
  kind: 'text';
  html: string;
}

export type Block = TextBlock;

/** What `GET /api/pages/:id` and `PUT /api/pages/:id` answer. */
export interface Page {
  id: string;
  blocks: Block[];
}

export type EventKind =
  | 'course'
  | 'syllabus'
  | 'page'
  | 'quiz'
  | 'debrief'
  | 'status'
  | 'thread'
  | 'reply'
  | 'choose'
  | 'finding'
  | 'cheatsheet'
  | 'idle'
  // The requests of the learner:
  | 'next'
  | 'answer'
  | 'ask'
  | 'chosen'
  | 'broken';

/**
 * One write of the server. Only the agent gets the `detail` of a request,
 * so the page never reads it.
 */
export interface Event {
  seq: number;
  kind: EventKind;
  author: 'learner' | 'agent';
  page: string | null;
  /** The quiz, the thread, or the choice. */
  id: string | null;
}

/** What `GET /api/events` answers. */
export interface EventBatch {
  events: Event[];
  /** The number to ask after next time. */
  next: number;
  /** The events asked for are gone: read everything again. */
  reset: boolean;
  /** An agent is present. */
  agent: boolean;
  /** The agent works on a request, and has not waited again since. */
  working: boolean;
  /** An agent was present on this server, and is present no more. */
  left: boolean;
  /** What the agent says it does, until its next wait. */
  status: string | null;
}

/** The body of every error of the API. */
export interface ErrorBody {
  error: { code: string; message: string };
}
