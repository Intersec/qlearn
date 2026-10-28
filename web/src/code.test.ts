import { describe, expect, it } from 'vitest';
import { pieces } from './code';

function span(start: number, end: number, cls = 'tok-keyword') {
  return { start, end, cls };
}

describe('pieces', () => {
  it('cuts a line at the bounds of its spans', () => {
    const line = { text: 'int x = 1;', spans: [span(0, 3), span(8, 9, 'tok-constant')] };

    expect(pieces(line)).toEqual([
      { text: 'int', cls: 'tok-keyword' },
      { text: ' x = ', cls: '' },
      { text: '1', cls: 'tok-constant' },
      { text: ';', cls: '' },
    ]);
  });

  it('gives the text back, whatever the spans', () => {
    const text = 'héllo 🚀 wörld';
    for (const spans of [
      [],
      [span(0, 100)],
      [span(3, 5), span(4, 9)],
      [span(8, 10), span(0, 2)],
      [span(5, 5), span(20, 30)],
    ]) {
      const out = pieces({ text, spans });
      expect(out.map((piece) => piece.text).join('')).toBe(text);
    }
  });

  it('slices in UTF-16 units, as the server counts them', () => {
    // The rocket is two units, so the word after it starts at 9.
    const out = pieces({ text: 'héllo 🚀 world', spans: [span(9, 14, 'tok-string')] });

    expect(out.at(-1)).toEqual({ text: 'world', cls: 'tok-string' });
  });

  it('has no piece for an empty line', () => {
    expect(pieces({ text: '', spans: [] })).toEqual([]);
  });
});
