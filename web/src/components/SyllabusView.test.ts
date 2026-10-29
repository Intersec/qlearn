// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import SyllabusView from './SyllabusView.vue';
import type { PageRef, Part } from '@/api/types';

function ref(id: string, fields: Partial<PageRef> = {}): PageRef {
  return {
    id,
    kind: id.startsWith('c') ? 'checkpoint' : 'lesson',
    title: `Page ${id}`,
    state: 'taught',
    score: null,
    ...fields,
  };
}

function view(parts: Part[], shown: string | null = null, current: string | null = null) {
  return mount(SyllabusView, { props: { parts, shown, current } });
}

describe('SyllabusView', () => {
  it('shows each part with its title and its pages in order', () => {
    const wrapper = view([
      { title: 'Why', pages: [ref('1'), ref('2')] },
      { title: 'How', pages: [ref('3'), ref('c1', { state: 'planned' })] },
    ]);

    expect(wrapper.findAll('.syllabus-part-title').map((title) => title.text())).toEqual([
      'Why',
      'How',
    ]);
    expect(wrapper.findAll('[data-page]').map((page) => page.attributes('data-page'))).toEqual([
      '1',
      '2',
      '3',
      'c1',
    ]);
    expect(wrapper.find('[data-page="3"] .syllabus-title').text()).toBe('Page 3');
  });

  it('shows the implicit part as a flat list', () => {
    const wrapper = view([{ title: '', pages: [ref('1'), ref('2')] }]);

    expect(wrapper.findAll('.syllabus-part-title')).toHaveLength(0);
    expect(wrapper.findAll('[data-page]')).toHaveLength(2);
  });

  it('says that the agent writes the syllabus when it holds no page', () => {
    for (const parts of [[], [{ title: '', pages: [] }]]) {
      const wrapper = view(parts);

      expect(wrapper.find('.syllabus-empty').exists()).toBe(true);
      expect(wrapper.findAll('[data-page]')).toHaveLength(0);
    }
  });

  it('shows a planned page, but not as a link', async () => {
    const wrapper = view([{ title: '', pages: [ref('1'), ref('2', { state: 'planned' })] }]);

    const planned = wrapper.find('[data-page="2"]');
    expect(planned.attributes('data-state')).toBe('planned');
    expect(planned.find('a').exists()).toBe(false);
    expect(planned.find('[aria-disabled="true"]').text()).toContain('Page 2');
    await planned.find('.syllabus-entry').trigger('click');
    expect(wrapper.emitted('open')).toBeUndefined();
  });

  it('opens a page with content', async () => {
    const wrapper = view([{ title: '', pages: [ref('1'), ref('2', { state: 'debriefed' })] }]);

    const link = wrapper.find('[data-page="2"] a');
    expect(link.attributes('href')).toBe('#page=2');
    await link.trigger('click');
    expect(wrapper.emitted('open')).toEqual([['2']]);
  });

  it('leaves a click with a modifier or another button to the browser, for a new tab', () => {
    const wrapper = view([{ title: '', pages: [ref('1'), ref('2')] }]);
    const link = wrapper.find('[data-page="2"] a').element;

    const clicks: MouseEventInit[] = [
      { ctrlKey: true },
      { metaKey: true },
      { shiftKey: true },
      { altKey: true },
      { button: 1 },
    ];
    for (const init of clicks) {
      const click = new MouseEvent('click', { bubbles: true, cancelable: true, ...init });
      expect(link.dispatchEvent(click), JSON.stringify(init)).toBe(true);
    }
    expect(wrapper.emitted('open')).toBeUndefined();

    const plain = new MouseEvent('click', { bubbles: true, cancelable: true });
    expect(link.dispatchEvent(plain)).toBe(false);
    expect(wrapper.emitted('open')).toEqual([['2']]);
  });

  it('gives each page its state', () => {
    const states = ['planned', 'taught', 'answered', 'debriefed'] as const;
    const wrapper = view([
      { title: '', pages: states.map((state, at) => ref(String(at + 1), { state })) },
    ]);

    const pages = wrapper.findAll('[data-page]');
    expect(pages.map((page) => page.attributes('data-state'))).toEqual([...states]);
    expect(pages.map((page) => page.find('.sr-only').text())).toEqual([
      '(Planned)',
      '(Taught)',
      '(Answered)',
      '(Debriefed)',
    ]);
  });

  it('gives a score in steps of a half, and none to a page with no score', () => {
    const wrapper = view([
      {
        title: '',
        pages: [
          ref('1', { state: 'answered', score: { right: 2, total: 3 } }),
          ref('2', { state: 'answered', score: { right: 1.5, total: 4 } }),
          ref('3', { state: 'debriefed', score: { right: 0, total: 2 } }),
          ref('4'),
        ],
      },
    ]);

    expect(wrapper.findAll('.syllabus-score').map((score) => score.text())).toEqual([
      '2/3',
      '1.5/4',
      '0/2',
    ]);
    expect(wrapper.find('[data-page="4"] .syllabus-score').exists()).toBe(false);
    expect(wrapper.find('[data-page="2"]').attributes('title')).toBe('Answered, 1.5/4 right');
  });

  it('marks the page on the screen and the current page', () => {
    const wrapper = view([{ title: '', pages: [ref('1'), ref('2'), ref('3')] }], '1', '3');

    const shown = wrapper.find('[data-page="1"]');
    expect(shown.attributes('data-shown')).toBe('true');
    expect(shown.find('a').attributes('aria-current')).toBe('page');
    expect(wrapper.findAll('[data-shown]')).toHaveLength(1);

    const current = wrapper.find('[data-page="3"]');
    expect(current.attributes('data-current')).toBe('true');
    expect(current.find('.syllabus-latest').text()).toBe('latest');
    expect(wrapper.findAll('.syllabus-latest')).toHaveLength(1);
    expect(wrapper.find('[data-page="2"] a').attributes('aria-current')).toBeUndefined();
  });

  it('puts the slot end after the pages, for the findings', () => {
    const wrapper = mount(SyllabusView, {
      props: { parts: [{ title: '', pages: [ref('1')] }], shown: null, current: null },
      slots: { end: '<section class="findings">Findings</section>' },
    });

    const nav = wrapper.get('nav').element;
    expect(nav.lastElementChild?.className).toBe('findings');
  });

  it('ends the pages with the cheatsheet, which opens once the agent wrote it', async () => {
    const parts = [{ title: '', pages: [ref('1')] }];
    const early = mount(SyllabusView, { props: { parts, shown: '1', current: '1' } });

    const waiting = early.get('.syllabus-sheet');
    expect(waiting.text()).toContain('Cheatsheet');
    expect(waiting.find('a').exists()).toBe(false);
    expect(waiting.get('[aria-disabled="true"]').attributes('title')).toBe(
      'The agent writes it at the end of the course',
    );

    const wrapper = mount(SyllabusView, {
      props: { parts, shown: null, current: '1', cheatsheet: true, sheetShown: true },
    });
    const link = wrapper.get('.syllabus-sheet a');
    expect(link.attributes('href')).toBe('#cheatsheet');
    expect(link.attributes('aria-current')).toBe('page');
    expect(wrapper.find('[data-page="1"] a').attributes('aria-current')).toBeUndefined();
    await link.trigger('click');
    expect(wrapper.emitted('cheatsheet')).toHaveLength(1);
  });

  it('shows no cheatsheet before the syllabus', () => {
    const wrapper = mount(SyllabusView, { props: { parts: [], shown: null, current: null } });

    expect(wrapper.find('.syllabus-sheet').exists()).toBe(false);
  });
});
