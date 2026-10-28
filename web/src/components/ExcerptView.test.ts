// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import ExcerptView from './ExcerptView.vue';
import type { Excerpt } from '@/api/types';

const COMMIT = '5aba991acd07dbe98266c4898e6bc5950240d118';

function excerpt(over: Partial<Excerpt> = {}): Excerpt {
  return {
    path: 'src/raft.c',
    repo: '',
    commit: COMMIT,
    from: 11,
    to: 14,
    lang: 'c',
    lines: [
      {
        text: 'int election_timeout(int base)',
        spans: [
          { start: 0, end: 3, cls: 'tok-storage tok-type' },
          { start: 4, end: 20, cls: 'tok-entity tok-name' },
        ],
      },
      { text: '{', spans: [] },
      null,
      { text: '', spans: [] },
    ],
    marked: [12, 13],
    ...over,
  };
}

function view(value: Excerpt = excerpt()) {
  return mount(ExcerptView, { props: { excerpt: value } });
}

describe('ExcerptView', () => {
  it('says where the lines come from, with the short hash', () => {
    const wrapper = view();

    expect(wrapper.find('.excerpt-path').text()).toBe('src/raft.c:11-14');
    const commit = wrapper.find('.excerpt-commit');
    expect(commit.text()).toBe('5aba991');
    expect(commit.attributes('title')).toBe(COMMIT);
    expect(wrapper.find('.excerpt-repo').exists()).toBe(false);
  });

  it('names the submodule that holds the file', () => {
    const wrapper = view(excerpt({ path: 'vendor/lib/src/lib.c', repo: 'vendor/lib' }));

    expect(wrapper.find('.excerpt-repo').text()).toBe('in vendor/lib');
  });

  it('says one line once', () => {
    const one = excerpt({ from: 7, to: 7, lines: [{ text: 'x;', spans: [] }], marked: [] });

    expect(view(one).find('.excerpt-path').text()).toBe('src/raft.c:7');
  });

  it('numbers the lines and marks the ones the lesson names', () => {
    const rows = view().findAll('tr');

    expect(rows.map((row) => row.find('.excerpt-number').text())).toEqual(['11', '12', '13', '14']);
    expect(rows.map((row) => row.classes('excerpt-marked'))).toEqual([false, true, true, false]);
  });

  it('paints the tokens with their classes and keeps the text whole', () => {
    const first = view().find('tr .excerpt-text');

    expect(first.text()).toBe('int election_timeout(int base)');
    expect(first.find('.tok-storage').text()).toBe('int');
    expect(first.find('.tok-entity').text()).toBe('election_timeout');
  });

  it('draws the hidden line as a blank, with no text', () => {
    const hidden = view().find('[data-line="13"] .excerpt-text');

    expect(hidden.find('.excerpt-hidden').exists()).toBe(true);
    expect(hidden.text()).toBe('');
  });

  // A copy of the code takes the text of each line, so an empty line holds
  // no character at all: a stray one breaks a compiler.
  it('leaves an empty line empty', () => {
    const empty = view().find('[data-line="14"] .excerpt-text');

    expect(empty.element.textContent).toBe('');
  });
});
