// @vitest-environment jsdom
// The Ask button of each block, and the threads that a page places.
import { describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import PageView from './PageView.vue';
import type { Page, Thread } from '@/api/types';

function threadOn(id: string, block: string | null): Thread {
  return {
    id,
    page: '1',
    block,
    quote: block ? `quote of ${id}` : null,
    comments: [{ author: 'learner', body: `<p>Question ${id}</p>`, at: '2026-01-01T00:00:00Z' }],
  };
}

function pageWith(threads: Thread[] = []): Page {
  return {
    id: '1',
    blocks: [
      {
        id: 'a',
        kind: 'text',
        html: '<h1>Two leaders</h1><p>Each server votes once per term.</p>',
      },
      { id: 'b', kind: 'text', html: '<p>The vote.</p>' },
    ],
    quizzes: [],
    threads,
    choices: [],
  };
}

/** Select `text` inside the first paragraph of the block `block`. */
function select(block: Element, text: string) {
  const node = block.querySelector('p')?.firstChild as Text;
  const start = node.data.indexOf(text);
  const range = document.createRange();
  range.setStart(node, start);
  range.setEnd(node, start + text.length);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
}

describe('the Ask button', () => {
  it('opens a question under its block, with the selection as its quote', async () => {
    const ask = vi.fn().mockResolvedValue(true);
    const wrapper = mount(PageView, { props: { page: pageWith(), ask }, attachTo: document.body });
    const anchor = wrapper.find('[data-anchor="a"]');

    select(anchor.find('[data-block="a"]').element, 'votes once');
    await anchor.find('.ask').trigger('mousedown');
    await anchor.find('.ask').trigger('click');

    expect(wrapper.emitted('focus')).toEqual([['a']]);
    const asking = anchor.find('.asking');
    expect(asking.find('.thread-quote').text()).toBe('votes once');
    expect(document.activeElement).toBe(asking.find('textarea').element);
    await asking.find('textarea').setValue('Once ever?');
    await asking.find('form').trigger('submit');
    await flushPromises();

    expect(ask).toHaveBeenCalledWith('a', 'votes once', 'Once ever?');
    expect(wrapper.find('.asking').exists()).toBe(false);
    wrapper.unmount();
  });

  it('takes no quote from a selection outside its block', async () => {
    const ask = vi.fn().mockResolvedValue(true);
    const wrapper = mount(PageView, { props: { page: pageWith(), ask }, attachTo: document.body });

    select(wrapper.find('[data-block="a"]').element, 'votes once');
    const other = wrapper.find('[data-anchor="b"]');
    await other.find('.ask').trigger('mousedown');
    await other.find('.ask').trigger('click');

    expect(other.find('.asking').exists()).toBe(true);
    expect(other.find('.thread-quote').exists()).toBe(false);
    await other.find('.asking textarea').setValue('Why?');
    await other.find('.asking form').trigger('submit');
    await flushPromises();
    expect(ask).toHaveBeenCalledWith('b', null, 'Why?');
    wrapper.unmount();
  });

  it('keeps the question when the server refuses it, and Cancel closes it', async () => {
    const ask = vi.fn().mockResolvedValue(false);
    const wrapper = mount(PageView, { props: { page: pageWith(), ask } });
    const anchor = wrapper.find('[data-anchor="b"]');

    await anchor.find('.ask').trigger('click');
    await anchor.find('.asking textarea').setValue('Why?');
    await anchor.find('.asking form').trigger('submit');
    await flushPromises();

    expect(anchor.find('.asking textarea').element).toHaveProperty('value', 'Why?');
    await anchor.find('.comment-cancel').trigger('click');
    expect(anchor.find('.asking').exists()).toBe(false);
  });

  it('takes the block out of the address on Cancel, unless a thread holds it', async () => {
    const wrapper = mount(PageView, { props: { page: pageWith([threadOn('1-1', 'a')]) } });

    for (const block of ['a', 'b']) {
      const anchor = wrapper.find(`[data-anchor="${block}"]`);
      await anchor.find('.ask').trigger('click');
      await anchor.find('.comment-cancel').trigger('click');
    }

    // A reload with the block of no thread would open the box again.
    expect(wrapper.emitted('focus')).toEqual([['a'], ['b'], [null]]);
  });

  it('opens the Ask box of the block that the address names', () => {
    const wrapper = mount(PageView, { props: { page: pageWith(), focus: 'b' } });

    expect(wrapper.find('[data-anchor="b"] .asking').exists()).toBe(true);
    expect(wrapper.find('[data-anchor="a"] .asking').exists()).toBe(false);
  });
});

describe('the threads of a page', () => {
  it('go under their block, about the page at the end, and those of a gone block last', () => {
    const page = pageWith([
      threadOn('1-1', 'gone'),
      threadOn('1-2', 'b'),
      threadOn('1-3', null),
      threadOn('1-4', 'b'),
    ]);
    const reply = vi.fn().mockResolvedValue(true);
    const wrapper = mount(PageView, { props: { page, reply } });

    const under = wrapper.findAll('[data-anchor="b"] .thread');
    expect(under.map((thread) => thread.attributes('data-thread'))).toEqual(['1-2', '1-4']);
    expect(wrapper.findAll('[data-anchor="a"] .thread')).toHaveLength(0);
    expect(wrapper.find('.threads-page .threads-head').text()).toBe('About this page');
    expect(wrapper.find('.threads-page .thread').attributes('data-thread')).toBe('1-3');
    const earlier = wrapper.find('.threads-earlier');
    expect(earlier.find('.threads-head').text()).toBe('On an earlier version of this page');
    expect(earlier.find('.thread').attributes('data-thread')).toBe('1-1');
    expect(earlier.find('.thread-quote').text()).toBe('quote of 1-1');
    // The earlier versions come after everything else of the page.
    const sections = wrapper.findAll('.lesson > *').map((node) => node.classes());
    expect(sections.at(-1)).toContain('threads-earlier');
  });

  it('go under a block of a debrief too', () => {
    const page = pageWith([threadOn('1-1', 'd')]);
    page.quizzes = [
      {
        open: null,
        choices: [],
        debrief: { blocks: [{ id: 'd', kind: 'text', html: '<p>Almost.</p>' }], points: [] },
      },
    ];
    const wrapper = mount(PageView, { props: { page } });

    expect(wrapper.find('.debrief [data-anchor="d"] .thread').exists()).toBe(true);
    expect(wrapper.find('.threads-earlier').exists()).toBe(false);
  });

  it('send a reply of the learner with the id of the thread', async () => {
    const reply = vi.fn().mockResolvedValue(true);
    const wrapper = mount(PageView, { props: { page: pageWith([threadOn('1-2', 'b')]), reply } });

    await wrapper.find('[data-thread="1-2"] textarea').setValue('And then?');
    await wrapper.find('[data-thread="1-2"] form').trigger('submit');
    await flushPromises();

    expect(reply).toHaveBeenCalledWith('1-2', 'And then?');
  });
});
