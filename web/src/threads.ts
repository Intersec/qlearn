// The threads of a page (section 6 of the design), as the blocks draw them.
//
// The page provides one context, and each list of blocks reads it: those
// of the lesson and those of a debrief take questions alike.

import type { InjectionKey, Ref } from 'vue';
import type { Block, Page, Thread } from './api/types';

/** The block whose Ask box is open, with the text selected in it. */
export interface Asking {
  block: string;
  quote: string | null;
}

export interface ThreadContext {
  /** The threads under a block, in the order that they opened. */
  of(block: string): Thread[];
  asking: Ref<Asking | null>;
  /** Open the Ask box under a block. */
  open(block: string, quote: string | null): void;
  /** Close the Ask box once its question is sent. */
  close(): void;
  /** Close the Ask box with no question. */
  cancel(): void;
  /** Send a question. Answers whether the server took it. */
  ask(block: string, quote: string | null, body: string): Promise<boolean>;
  /** Write in a thread. Answers whether the server took it. */
  reply(thread: string, body: string): Promise<boolean>;
}

export const THREADS: InjectionKey<ThreadContext> = Symbol('threads');

/**
 * The text that the learner selected inside `element`, or null. A
 * selection that leaves the block is no quote of it.
 */
export function quoteIn(element: Element | null): string | null {
  const selection = window.getSelection();
  if (!element || !selection || selection.rangeCount === 0 || selection.isCollapsed) {
    return null;
  }
  const range = selection.getRangeAt(0);
  if (!element.contains(range.startContainer) || !element.contains(range.endContainer)) {
    return null;
  }
  const text = selection.toString().trim();
  return text === '' ? null : text;
}

/** The threads of a page, by where the page shows them. */
export interface Placed {
  /** Under each block of the lesson or of a debrief. */
  byBlock: Map<string, Thread[]>;
  /** About the whole page, at its end. */
  whole: Thread[];
  /** On a block that a later push took away, at the very end. */
  earlier: Thread[];
}

export function place(page: Page): Placed {
  const blocks: Block[] = [
    ...page.blocks,
    ...page.quizzes.flatMap((quiz) => quiz.debrief?.blocks ?? []),
  ];
  const held = new Set(blocks.map((block) => block.id));
  const placed: Placed = { byBlock: new Map(), whole: [], earlier: [] };

  for (const thread of page.threads) {
    if (thread.block === null) {
      placed.whole.push(thread);
    } else if (held.has(thread.block)) {
      const under = placed.byBlock.get(thread.block) ?? [];
      under.push(thread);
      placed.byBlock.set(thread.block, under);
    } else {
      placed.earlier.push(thread);
    }
  }
  return placed;
}
