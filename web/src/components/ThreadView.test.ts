// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import ThreadView from './ThreadView.vue';
import type { Thread } from '@/api/types';

function thread(fields: Partial<Thread> = {}): Thread {
  return {
    id: '1-1',
    page: '1',
    block: 'block0',
    quote: 'votes once',
    comments: [
      { author: 'learner', body: '<p>Once per term?</p>', at: '2026-01-01T00:00:00Z' },
      {
        author: 'agent',
        body: '<p>Once per <strong>term</strong>.</p><img src="x" onerror="window.ran = 1">',
        at: '2026-01-01T00:01:00Z',
      },
    ],
    ...fields,
  };
}

describe('ThreadView', () => {
  it('shows the quote, then each comment with its author', () => {
    const wrapper = mount(ThreadView, { props: { thread: thread(), reply: async () => true } });

    expect(wrapper.find('.thread').attributes('data-thread')).toBe('1-1');
    expect(wrapper.find('.thread-quote').text()).toBe('votes once');
    const comments = wrapper.findAll('.thread-comment');
    expect(comments.map((comment) => comment.attributes('data-author'))).toEqual([
      'learner',
      'agent',
    ]);
    expect(comments.map((comment) => comment.find('.thread-who').text())).toEqual(['You', 'Agent']);
    expect(comments[1].find('.thread-body strong').text()).toBe('term');
    // The agent answered: nothing waits.
    expect(wrapper.find('.thread-waiting').exists()).toBe(false);
  });

  it('passes the HTML of the server through DOMPurify', () => {
    const wrapper = mount(ThreadView, { props: { thread: thread(), reply: async () => true } });

    expect(wrapper.html()).not.toContain('onerror');
  });

  it('says that the agent owes the next comment, and shows no quote when none', () => {
    const asked = thread({ quote: null, comments: [thread().comments[0]] });
    const wrapper = mount(ThreadView, { props: { thread: asked, reply: async () => true } });

    expect(wrapper.find('.thread-quote').exists()).toBe(false);
    expect(wrapper.find('.thread-waiting').text()).toBe('The agent writes its answer here.');
    // A second message before the answer would only ask again.
    expect(wrapper.find('textarea').exists()).toBe(false);
  });

  it('sends a reply, and keeps the text when the server refuses it', async () => {
    const reply = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    const wrapper = mount(ThreadView, { props: { thread: thread(), reply } });
    const box = wrapper.find('textarea');
    const send = wrapper.find('.comment-send');
    expect(send.attributes('disabled')).toBeDefined();

    await box.setValue('And after a restart?');
    await wrapper.find('form').trigger('submit');
    await flushPromises();
    expect(reply).toHaveBeenLastCalledWith('And after a restart?');
    expect((box.element as HTMLTextAreaElement).value).toBe('And after a restart?');

    await wrapper.find('form').trigger('submit');
    await flushPromises();
    expect(reply).toHaveBeenCalledTimes(2);
    expect((box.element as HTMLTextAreaElement).value).toBe('');
  });
});
