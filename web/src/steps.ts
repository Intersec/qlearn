// The cells of a step that changed since the previous step (section 4.5 of
// the design). The rule is in the entry "Holes found by the steps" of
// roadmap/stack.md.

import type { Step } from '@/api/types';

/** A cell as the page marks it. `was` is null in a row that is new. */
export interface Change {
  changed: boolean;
  was: string | null;
}

/**
 * The change of each cell of step `at`, against the nearest earlier step
 * that holds rows: a diagram step between two tables hides no change. Rows
 * compare by index, so a row past the end of the earlier table is new.
 * The first table marks nothing.
 */
export function changes(steps: Step[], at: number): Change[][] {
  const rows = steps[at]?.rows;
  if (!rows) {
    return [];
  }
  const before = steps
    .slice(0, at)
    .reverse()
    .find((step) => step.rows !== null)?.rows;

  return rows.map((row, r) =>
    row.map((cell, c) => {
      if (!before) {
        return { changed: false, was: null };
      }
      const old = before[r];
      if (!old) {
        return { changed: true, was: null };
      }
      const was = old[c] ?? '';
      return { changed: was !== cell, was };
    }),
  );
}
