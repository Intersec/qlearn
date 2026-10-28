import { describe, expect, it } from 'vitest';
import { format, parse } from './place';

describe('format', () => {
  it('names the page alone when no thread is open', () => {
    expect(format({ page: '7a' })).toBe('#page=7a');
  });

  it('adds the block whose thread is open', () => {
    expect(format({ page: 'c1', block: '3f9c2e81d0b4' })).toBe('#page=c1&block=3f9c2e81d0b4');
  });
});

describe('parse', () => {
  it('names no place in an empty fragment', () => {
    expect(parse('')).toBeNull();
    expect(parse('#')).toBeNull();
  });

  it('names no place without a page', () => {
    expect(parse('#block=3f9c2e81d0b4')).toBeNull();
  });

  it('reads a page alone', () => {
    expect(parse('#page=1')).toEqual({ page: '1', block: undefined });
  });

  it('reads a page and a block', () => {
    expect(parse('#page=1&block=3f9c2e81d0b4')).toEqual({ page: '1', block: '3f9c2e81d0b4' });
  });

  it('ignores the keys it does not know', () => {
    expect(parse('#page=1&change=I8f3a&ps=2')).toEqual({ page: '1', block: undefined });
  });

  it('reads back what format wrote', () => {
    const place = { page: 'f1', block: '3f9c2e81d0b4' };

    expect(parse(format(place))).toEqual(place);
  });
});
