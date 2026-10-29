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
  /** In steps of 0.5: a partial result counts half. */
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
  /**
   * The choices that name no page, open or answered, in the order that the
   * agent asked them. They show at the top of the syllabus column.
   */
  choices: Choosing[];
  /**
   * Why the server cannot read `choices.json`, or null. The choices then
   * show nowhere, and only the routes of the choices refuse.
   */
  choicesError: string | null;
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

/** The marker on the first line of a callout, such as `[!hook]`. */
export type Marker = 'hook' | 'teaser' | 'pitfall' | 'note';

/** A quote that opens with a marker. The HTML holds no marker: the page draws it. */
export interface CalloutBlock {
  id: string;
  kind: 'callout';
  marker: Marker;
  html: string;
}

/** Lines of the repository, read by the server at the push. */
export interface ExcerptBlock {
  id: string;
  kind: 'excerpt';
  excerpt: Excerpt;
}

/**
 * A Mermaid source, as the agent wrote it. The page draws it, because the
 * server cannot run Mermaid, and reports a source that Mermaid refuses.
 */
export interface DiagramBlock {
  id: string;
  kind: 'diagram';
  source: string;
}

export type Block = TextBlock | CalloutBlock | ExcerptBlock | DiagramBlock;

export interface Excerpt {
  /** From the top level directory of the worktree. */
  path: string;
  /** "" for the main repository, or the path of the submodule. */
  repo: string;
  /** The full hash, in the repository that holds the file. */
  commit: string;
  from: number;
  to: number;
  /** The token that named the grammar, such as `c`, or "" for plain text. */
  lang: string;
  /** null: the hidden line, before the answer. */
  lines: (Line | null)[];
  marked: number[];
}

/** A line of code with its syntax classes. */
export interface Line {
  text: string;
  spans: Span[];
}

/** A range of a line with its syntax class, in UTF-16 code units of the line. */
export interface Span {
  start: number;
  end: number;
  /** One or two classes, such as `tok-keyword tok-control`. */
  cls: string;
}

/** What `GET /api/pages/:id` and `PUT /api/pages/:id` answer. */
export interface Page {
  id: string;
  blocks: Block[];
  /** In the order that the agent added them. Quiz `n` is at `n - 1`. */
  quizzes: Quiz[];
  /** In the order that they opened. A thread whose block is gone comes last. */
  threads: Thread[];
  /** The choices that the agent asked at the end of this page, open or answered. */
  choices: Choosing[];
}

/**
 * A choice that the agent asks the learner, in place of a dialog of its
 * terminal (section 7 of the design). It is never graded.
 */
export interface Choosing {
  /** `choice-<n>`, such as `choice-2`. */
  id: string;
  /** The page at whose end it shows, or null for the top of the syllabus column. */
  page: string | null;
  questions: ChoosingQuestion[];
  /** null until the learner answers. Then one answer per question, in order. */
  answers: ChosenAnswer[] | null;
}

/** A question of a choice. Every text is plain text. */
export interface ChoosingQuestion {
  /** A short name of the question. */
  header: string;
  prompt: string;
  options: ChoosingOption[];
  /** The learner may pick several options. */
  multi: boolean;
}

export interface ChoosingOption {
  label: string;
  /** "" when the agent gave none. */
  description: string;
}

/** The answer of the learner to one question of a choice, as the page sends it. */
export interface ChosenAnswer {
  /** The options picked, from 0, in order. Empty when the free text answers alone. */
  picked: number[];
  /** The free text, or null. */
  other: string | null;
}

/** A question of the learner, or a word of the agent (section 6 of the design). */
export interface Thread {
  /** `<page>-<n>`, such as `3-2`. */
  id: string;
  page: string;
  /** null: about the whole page. */
  block: string | null;
  /** The text of the block that the learner selected. */
  quote: string | null;
  comments: Comment[];
}

export interface Comment {
  author: 'learner' | 'agent';
  /** The HTML that the server writes from the Markdown. */
  body: string;
  /** RFC 3339. */
  at: string;
}

/**
 * A quiz as the page sees it (section 5.2 of the design). The right option
 * of a choice stays on the server until the learner sends the choices.
 */
export interface Quiz {
  open: OpenQuestion | null;
  choices: Choice[];
  /** null until the agent writes it, once the learner answered the quiz. */
  debrief: Debrief | null;
}

/** What the agent says of the answers to a quiz (section 5.4 of the design). */
export interface Debrief {
  /** The blocks of its source, as the blocks of a page. No line is hidden. */
  blocks: Block[];
  /** The grade of each point of the open question, in its order. */
  points: GradedPoint[];
}

/** `partial` is an open answer that is half right. A choice is `ok` or `missed`. */
export type Grade = 'ok' | 'partial' | 'missed';

/** A point of an open question, with the grade that the agent gave it. */
export interface GradedPoint {
  id: string;
  label: string;
  result: Grade;
}

export interface OpenQuestion {
  /** Plain text. */
  prompt: string;
  /** The open answer of the learner, once sent. */
  sent: string | null;
}

/** A question with options, of which one is right. */
export interface Choice {
  /** Plain text: a short name of the question. */
  header: string;
  /** Plain text. */
  prompt: string;
  /** The HTML of each option, written by the server. */
  options: string[];
  /** null until the learner sends the choices: it holds the right option. */
  graded: GradedChoice | null;
}

/** A choice once the learner has sent the choices of its quiz. */
export interface GradedChoice {
  /** The option that the learner picked, from 0. */
  picked: number;
  /** The right option, from 0. */
  answer: number;
  ok: boolean;
  /** The text of the "Other" box, or null when the learner left it empty. */
  other: string | null;
}

/** The pick of the learner for one choice, as the page sends it. */
export interface Pick {
  /** From 0. */
  option: number;
  other: string | null;
}

/** A part of the answer to a quiz: the open answer, or the choices. */
export type QuizAnswer = { open: string } | { choices: Pick[] };

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
  /** That work holds a Next request: the agent writes a page. */
  preparing: boolean;
  /** An agent was present on this server, and is present no more. */
  left: boolean;
  /** What the agent says it does, until its next wait. */
  status: string | null;
}

/** The body of every error of the API. */
export interface ErrorBody {
  error: { code: string; message: string };
}
