// @vitest-environment jsdom
// Mermaid is mocked at the module boundary, as in DiagramView.test.ts.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils';
import CheatsheetView from './CheatsheetView.vue';
import type { Cheatsheet, Course } from '@/api/types';

const mermaid = vi.hoisted(() => ({
  initialize: vi.fn(),
  parse: vi.fn(),
  render: vi.fn(),
}));

vi.mock('mermaid', () => ({ default: mermaid }));

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.resetAllMocks();
});

function course(fields: Partial<Course> = {}): Course {
  return {
    version: 1,
    slug: 'raft',
    title: 'Raft',
    goal: '',
    profile: '',
    quizMode: 'mixed',
    status: 'running',
    followUp: null,
    parts: [],
    findings: [],
    reading: null,
    created: '2026-01-01T00:00:00Z',
    updated: '2026-01-01T00:00:00Z',
    ...fields,
  };
}

function sheet(fields: Partial<Cheatsheet> = {}): Cheatsheet {
  return {
    written: true,
    blocks: [
      { id: 'title', kind: 'text', html: '<h1>Raft in one page</h1>' },
      { id: 'hook', kind: 'callout', marker: 'hook', html: '<p>The first to wake up wins.</p>' },
    ],
    map: [
      {
        path: 'src/raft.c',
        repo: '',
        commit: '5aba991acd07dbe98266c4898e6bc5950240d118',
        from: 1,
        to: 6,
        lessons: [
          { id: '1', title: 'The timer' },
          { id: '2', title: 'The vote' },
        ],
      },
      {
        path: 'lib/list.c',
        repo: 'lib',
        commit: '0f0b5a6e7d978a501a71e918f8e9060b95427039',
        from: 3,
        to: 3,
        lessons: [{ id: '2', title: 'The vote' }],
      },
    ],
    revisit: [
      {
        point: 'split-vote',
        label: 'How a split vote ends',
        result: 'missed',
        lesson: '1',
        title: 'The timer',
      },
      {
        point: 'majority',
        label: 'How many votes win',
        result: 'partial',
        lesson: '9',
        title: null,
      },
    ],
    ...fields,
  };
}

describe('CheatsheetView', () => {
  it('lays the blocks of the agent, then the map of the code, then the points to revisit', () => {
    const wrapper = mount(CheatsheetView, { props: { cheatsheet: sheet(), course: course() } });

    expect(wrapper.get('.cheatsheet-sheet h1').text()).toBe('Raft in one page');
    expect(wrapper.find('.callout').exists()).toBe(true);
    expect(wrapper.findAll('.cheatsheet-head').map((head) => head.text())).toEqual([
      'Map of the code',
      'To revisit',
    ]);

    const ranges = wrapper.findAll('.cheatsheet-range');
    expect(ranges.map((range) => range.get('.cheatsheet-where').text())).toEqual([
      'src/raft.c:1-6',
      'lib/list.c:3',
    ]);
    expect(ranges[0].get('.cheatsheet-commit').text()).toBe('5aba991');
    expect(ranges[0].find('.cheatsheet-repo').exists()).toBe(false);
    expect(ranges[1].get('.cheatsheet-repo').text()).toBe('in lib');
    expect(ranges[0].findAll('.cheatsheet-lesson').map((lesson) => lesson.text())).toEqual([
      '1 The timer',
      '2 The vote',
    ]);

    const points = wrapper.findAll('.cheatsheet-point');
    expect(points.map((point) => point.attributes('data-result'))).toEqual(['missed', 'partial']);
    expect(points[0].text()).toContain('How a split vote ends');
    expect(points[0].get('.cheatsheet-result').text()).toBe('Missed');
    expect(points[1].get('.cheatsheet-result').text()).toBe('Partial');
    // A page that the syllabus does not hold is named, and opens nothing.
    expect(points[1].find('a').exists()).toBe(false);
    expect(points[1].get('.cheatsheet-lesson').text()).toBe('9');
  });

  it('takes no question: it is not a page of the course', () => {
    const wrapper = mount(CheatsheetView, { props: { cheatsheet: sheet(), course: course() } });

    expect(wrapper.find('.ask').exists()).toBe(false);
    expect(wrapper.findAll('button').map((button) => button.text())).toEqual([
      'Print or save as PDF',
      'Save as HTML',
    ]);
  });

  it('opens the page of a lesson that it names', async () => {
    const wrapper = mount(CheatsheetView, { props: { cheatsheet: sheet(), course: course() } });

    const link = wrapper.get('.cheatsheet-range .cheatsheet-lesson');
    expect(link.attributes('href')).toBe('#page=1');
    await link.trigger('click');

    expect(wrapper.emitted('open')).toEqual([['1']]);
  });

  it('says what an empty section holds', () => {
    const wrapper = mount(CheatsheetView, {
      props: { cheatsheet: sheet({ map: [], revisit: [] }), course: course() },
    });

    expect(wrapper.findAll('.cheatsheet-none').map((none) => none.text())).toEqual([
      'The course showed no code.',
      'Nothing to revisit.',
    ]);
  });

  it('says, once the course is finished, that deleting it deletes the cheatsheet', () => {
    const running = mount(CheatsheetView, { props: { cheatsheet: sheet(), course: course() } });
    const finished = mount(CheatsheetView, {
      props: { cheatsheet: sheet(), course: course({ status: 'finished' }) },
    });

    expect(running.find('.cheatsheet-keep').exists()).toBe(false);
    const note = finished.get('.cheatsheet-keep').text();
    expect(note).toContain('.learn/raft/');
    expect(note).toContain('Deleting the course deletes it');
    expect(note).toContain('Save as PDF');
    expect(note).toContain('HTML');
    expect(note).toContain('Markdown');
  });

  it('promises no fix of a diagram that Mermaid refuses: no request reaches the agent', async () => {
    mermaid.parse.mockRejectedValue(new Error('Parse error on line 2'));
    const blocks: Cheatsheet['blocks'] = [
      { id: 'flow', kind: 'diagram', source: 'graph LR\n  A -->\n' },
      {
        id: 'vote',
        kind: 'steps',
        steps: {
          title: 'A vote',
          columns: [],
          steps: [
            { label: 't = 0', html: 'S1 wakes.', rows: null, diagram: 'graph LR\n  B -->\n' },
          ],
        },
      },
    ];
    const wrapper = mount(CheatsheetView, {
      props: { cheatsheet: sheet({ blocks }), course: course() },
    });
    await flushPromises();

    const heads = wrapper.findAll('.diagram-broken-head').map((head) => head.text());
    expect(heads).toEqual([
      'Mermaid cannot draw this diagram.',
      'Mermaid cannot draw this diagram.',
    ]);
    expect(wrapper.findAll('.diagram-error').map((error) => error.text())).toEqual([
      'Parse error on line 2',
      'Parse error on line 2',
    ]);
  });

  it('saves the Markdown that the server writes, under the name of the course', () => {
    const wrapper = mount(CheatsheetView, { props: { cheatsheet: sheet(), course: course() } });

    const link = wrapper.get('a.cheatsheet-markdown');
    expect(link.text()).toBe('Save as Markdown');
    expect(link.attributes('href')).toBe('/api/cheatsheet.md');
    expect(link.attributes('download')).toBe('raft-cheatsheet.md');
  });

  it('saves the sheet as one HTML file, under the name of the course', async () => {
    let saved: Blob | null = null;
    const created = vi.fn((blob: Blob) => {
      saved = blob;
      return 'blob:sheet';
    });
    // jsdom has no object URL: the browser tests save a real file.
    Object.assign(URL, { createObjectURL: created, revokeObjectURL: vi.fn() });
    const clicked = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      expect(this.href).toBe('blob:sheet');
      expect(this.download).toBe('raft-cheatsheet.html');
    });
    const wrapper = mount(CheatsheetView, { props: { cheatsheet: sheet(), course: course() } });

    await wrapper.get('.cheatsheet-html').trigger('click');
    await flushPromises();

    expect(clicked).toHaveBeenCalledOnce();
    expect(saved!.type).toBe('text/html;charset=utf-8');
    const html = await saved!.text();
    expect(html).toContain('<title>Cheatsheet: Raft</title>');
    expect(html).toContain('Raft in one page');
    expect(wrapper.find('.cheatsheet-failed').exists()).toBe(false);
    clicked.mockRestore();
  });

  it('shows the steps of a block all at once', async () => {
    const blocks: Cheatsheet['blocks'] = [
      {
        id: 'vote',
        kind: 'steps',
        steps: {
          title: 'A vote',
          columns: ['Server'],
          steps: [
            { label: 't = 0', html: 'S1 wakes.', rows: [['S1']], diagram: null },
            { label: 't = 1', html: 'S2 wakes.', rows: [['S2']], diagram: null },
          ],
        },
      },
    ];
    const wrapper = mount(CheatsheetView, {
      props: { cheatsheet: sheet({ blocks }), course: course() },
    });
    await flushPromises();

    expect(wrapper.findAll('.steps-step .steps-body').map((body) => body.text())).toEqual([
      'S1 wakes.',
      'S2 wakes.',
    ]);
    expect(wrapper.find('.steps-player').exists()).toBe(false);
  });

  it('prints with the print dialog of the browser', async () => {
    const print = vi.spyOn(window, 'print').mockImplementation(() => {});
    const wrapper = mount(CheatsheetView, { props: { cheatsheet: sheet(), course: course() } });

    await wrapper.get('.cheatsheet-print').trigger('click');

    expect(print).toHaveBeenCalledOnce();
    print.mockRestore();
  });
});
