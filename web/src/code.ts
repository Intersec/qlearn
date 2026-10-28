// A line of code, cut into the pieces that a template paints.
//
// The server sends the syntax classes as spans of the line, in UTF-16
// units, which is what a string of the browser slices with.

import type { Line } from '@/api/types';

export interface Piece {
  text: string;
  /** The syntax classes, empty when no span claims the piece. */
  cls: string;
}

/**
 * The pieces of a line, in order. Joined, they give the text back: a span
 * that leaves the line or overlaps the one before is cut, never repeated.
 */
export function pieces(line: Line): Piece[] {
  const { text } = line;
  const out: Piece[] = [];
  let last = 0;

  for (const span of [...line.spans].sort((a, b) => a.start - b.start)) {
    const start = Math.max(span.start, last);
    const end = Math.min(span.end, text.length);
    if (start >= end) {
      continue;
    }
    if (start > last) {
      out.push({ text: text.slice(last, start), cls: '' });
    }
    out.push({ text: text.slice(start, end), cls: span.cls });
    last = end;
  }
  if (last < text.length) {
    out.push({ text: text.slice(last), cls: '' });
  }
  return out;
}
