// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import PageView from './PageView.vue';
import type { Page } from '@/api/types';

// The real Mermaid runs in the browser tests.
vi.mock('mermaid', () => ({
  default: {
    initialize: () => {},
    parse: async (source: string) => {
      if (source.includes('-->\n')) {
        throw new Error('Parse error on line 2');
      }
      return { diagramType: 'flowchart-v2' };
    },
    render: async () => ({ svg: '<svg xmlns="http://www.w3.org/2000/svg"><text>A</text></svg>' }),
  },
}));

function view(...html: string[]) {
  const page: Page = {
    id: '1',
    blocks: html.map((text, at) => ({ id: `block${at}`, kind: 'text', html: text })),
    threads: [],
    choices: [],
    quizzes: [],
  };
  return mount(PageView, { props: { page } });
}

describe('PageView', () => {
  it('shows the blocks in order, each with its id', () => {
    const wrapper = view('<h1>Two leaders</h1>', '<h2>The vote</h2>');

    const blocks = wrapper.findAll('[data-block]');
    expect(blocks.map((block) => block.attributes('data-block'))).toEqual(['block0', 'block1']);
    expect(blocks.map((block) => block.text())).toEqual(['Two leaders', 'The vote']);
  });

  it('passes the HTML of the server through DOMPurify', () => {
    const wrapper = view(
      '<p>kept</p><script>window.ran = true</script><img src="x" onerror="window.ran = true">',
    );

    const html = wrapper.html();
    expect(html).toContain('<p>kept</p>');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('onerror');
  });

  it('keeps the colors of a fence', () => {
    const wrapper = view(
      '<pre class="md-code"><code><span class="tok-keyword tok-control">return</span></code></pre>',
    );

    expect(wrapper.find('.md-code .tok-keyword').text()).toBe('return');
  });

  it('draws a callout between the text blocks', () => {
    const page: Page = {
      id: '1',
      blocks: [
        { id: 'a', kind: 'text', html: '<p>Before.</p>' },
        { id: 'b', kind: 'callout', marker: 'teaser', html: '<p>Lesson 4 shows it.</p>' },
        { id: 'c', kind: 'text', html: '<p>After.</p>' },
      ],
      threads: [],
      choices: [],
      quizzes: [],
    };
    const wrapper = mount(PageView, { props: { page } });

    const blocks = wrapper.findAll('[data-block]');
    expect(blocks.map((block) => block.attributes('data-block'))).toEqual(['a', 'b', 'c']);
    expect(blocks[1].find('.callout').attributes('data-marker')).toBe('teaser');
    expect(blocks[1].find('.callout-body').text()).toBe('Lesson 4 shows it.');
  });

  it('draws an excerpt between the text blocks', () => {
    const page: Page = {
      id: '1',
      blocks: [
        { id: 'a', kind: 'text', html: '<p>Before.</p>' },
        {
          id: 'b',
          kind: 'excerpt',
          excerpt: {
            path: 'src/raft.c',
            repo: '',
            commit: '5aba991acd07dbe98266c4898e6bc5950240d118',
            from: 1,
            to: 1,
            lang: 'c',
            lines: [{ text: 'int x;', spans: [] }],
            marked: [],
          },
        },
        { id: 'c', kind: 'text', html: '<p>After.</p>' },
      ],
      threads: [],
      choices: [],
      quizzes: [],
    };
    const wrapper = mount(PageView, { props: { page } });

    const blocks = wrapper.findAll('[data-block]');
    expect(blocks.map((block) => block.attributes('data-block'))).toEqual(['a', 'b', 'c']);
    expect(blocks[1].find('.excerpt-path').text()).toBe('src/raft.c:1');
    expect(blocks[1].find('.excerpt-text').text()).toBe('int x;');
  });

  it('draws a diagram between the text blocks', async () => {
    const page: Page = {
      id: '1',
      blocks: [
        { id: 'a', kind: 'text', html: '<p>Before.</p>' },
        { id: 'b', kind: 'diagram', source: 'graph LR\n  A --> B\n' },
        { id: 'c', kind: 'text', html: '<p>After.</p>' },
      ],
      threads: [],
      choices: [],
      quizzes: [],
    };
    const wrapper = mount(PageView, { props: { page } });
    await flushPromises();

    const blocks = wrapper.findAll('[data-block]');
    expect(blocks.map((block) => block.attributes('data-block'))).toEqual(['a', 'b', 'c']);
    expect(blocks[1].find('.diagram-svg text').text()).toBe('A');
    expect(wrapper.emitted('broken')).toBeUndefined();
  });

  it('shows the quizzes under the lesson, in order', () => {
    const page: Page = {
      id: '1',
      blocks: [{ id: 'a', kind: 'text', html: '<p>The lesson.</p>' }],
      threads: [],
      choices: [],
      quizzes: [
        { open: { prompt: 'Why?', sent: null }, choices: [], debrief: null },
        {
          open: null,
          choices: [{ header: 'Vote', prompt: 'Who?', options: ['S1', 'S2'], graded: null }],
          debrief: null,
        },
      ],
    };
    const wrapper = mount(PageView, { props: { page } });

    const parts = wrapper.findAll('[data-block], [data-quiz]');
    expect(
      parts.map((part) => part.attributes('data-block') ?? part.attributes('data-quiz')),
    ).toEqual(['a', '1', '2']);
    expect(parts[1].find('.quiz-open').exists()).toBe(true);
    expect(parts[2].find('.quiz-choices').exists()).toBe(true);
  });

  it('sends an answer with the number of its quiz', async () => {
    const page: Page = {
      id: '1',
      blocks: [],
      threads: [],
      choices: [],
      quizzes: [
        { open: { prompt: 'Why?', sent: null }, choices: [], debrief: null },
        { open: { prompt: 'How?', sent: null }, choices: [], debrief: null },
      ],
    };
    const send = vi.fn(async () => true);
    const wrapper = mount(PageView, { props: { page, send } });

    await wrapper.findAll('textarea')[1].setValue('Like this.');
    await wrapper.findAll('form')[1].trigger('submit');

    expect(send).toHaveBeenCalledWith(2, { open: 'Like this.' });
  });

  it('leaves the draft of a quiz on its page when another page opens', async () => {
    const lesson = (id: string, prompt: string): Page => ({
      id,
      blocks: [{ id: `text${id}`, kind: 'text', html: '<p>The lesson.</p>' }],
      threads: [],
      choices: [],
      quizzes: [
        { open: { prompt, sent: null }, choices: [], debrief: null },
        {
          open: null,
          choices: [{ header: 'Vote', prompt: 'Who?', options: ['S1', 'S2'], graded: null }],
          debrief: null,
        },
      ],
    });
    const wrapper = mount(PageView, { props: { page: lesson('1', 'Why 1?') } });
    await wrapper.find('textarea').setValue('my answer to page 1');
    await wrapper.findAll('input[type="radio"]')[1].setValue(true);

    await wrapper.setProps({ page: lesson('2', 'Why 2?') });

    expect(wrapper.find('.quiz-prompt').text()).toBe('Why 2?');
    expect((wrapper.find('textarea').element as HTMLTextAreaElement).value).toBe('');
    const radios = wrapper.findAll('input[type="radio"]');
    expect(radios.map((radio) => (radio.element as HTMLInputElement).checked)).toEqual([
      false,
      false,
    ]);
  });

  it('names its page when a diagram of a debrief cannot be drawn', async () => {
    const page: Page = {
      id: '3',
      blocks: [{ id: 'a', kind: 'text', html: '<p>The lesson.</p>' }],
      threads: [],
      choices: [],
      quizzes: [
        {
          open: { prompt: 'Why?', sent: 'Because.' },
          choices: [],
          debrief: {
            blocks: [{ id: 'd', kind: 'diagram', source: 'graph LR\n  A -->\n' }],
            points: [{ id: 'why', label: 'Why', result: 'ok' }],
          },
        },
      ],
    };
    const wrapper = mount(PageView, { props: { page } });
    await flushPromises();

    expect(wrapper.findAll('[data-block]').map((block) => block.attributes('data-block'))).toEqual([
      'a',
      'd',
    ]);
    expect(wrapper.emitted('broken')).toEqual([['3', 'd', 'Parse error on line 2']]);
  });

  it('names its page when a diagram cannot be drawn', async () => {
    const page: Page = {
      id: '7a',
      blocks: [{ id: 'b', kind: 'diagram', source: 'graph LR\n  A -->\n' }],
      threads: [],
      choices: [],
      quizzes: [],
    };
    const wrapper = mount(PageView, { props: { page } });
    await flushPromises();

    expect(wrapper.emitted('broken')).toEqual([['7a', 'b', 'Parse error on line 2']]);
  });
});
