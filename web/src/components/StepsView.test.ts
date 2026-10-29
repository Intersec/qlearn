// @vitest-environment jsdom
// Mermaid is mocked at the module boundary, as in DiagramView.test.ts: the
// real one runs in the browser tests.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils';
import StepsView from './StepsView.vue';
import type { Steps } from '@/api/types';
import { changes } from '@/steps';

const mermaid = vi.hoisted(() => ({
  initialize: vi.fn(),
  parse: vi.fn(),
  render: vi.fn(),
}));

vi.mock('mermaid', () => ({ default: mermaid }));

enableAutoUnmount(afterEach);

const VOTE: Steps = {
  title: 'Two candidates in term 6',
  columns: ['Server', 'Term', 'Voted for'],
  steps: [
    {
      label: 't = 0 ms',
      html: 'S1 <em>times out</em>.',
      rows: [
        ['S1', '6', 'S1'],
        ['S2', '5', ''],
      ],
      diagram: null,
    },
    {
      label: 't = 2 ms',
      html: 'S2 votes.',
      rows: [
        ['S1', '6', 'S1'],
        ['S2', '6', 'S1'],
      ],
      diagram: null,
    },
    {
      label: 't = 3 ms',
      html: 'S1 asks S3.',
      rows: null,
      diagram: 'sequenceDiagram\n  S1->>S3: vote?\n',
    },
    {
      label: 't = 4 ms',
      html: 'S3 joins.',
      rows: [
        ['S1', '6', 'S1'],
        ['S2', '6', 'S1'],
        ['S3', '6', 'S1'],
      ],
      diagram: null,
    },
  ],
};

const SVG = '<svg xmlns="http://www.w3.org/2000/svg"><g><text>S3</text></g></svg>';

async function view(steps: Steps = VOTE) {
  const wrapper = mount(StepsView, { props: { id: 's1', steps }, attachTo: document.body });
  await flushPromises();
  return wrapper;
}

type Wrapper = Awaited<ReturnType<typeof view>>;

function counter(wrapper: Wrapper): string {
  return wrapper.find('.steps-count [aria-hidden="true"]').text();
}

function changed(wrapper: Wrapper): string[] {
  return wrapper.findAll('td.steps-changed').map((cell) => cell.text());
}

async function next(wrapper: Wrapper) {
  await wrapper.get('button[aria-label="Next step"]').trigger('click');
  await flushPromises();
}

beforeEach(() => {
  mermaid.parse.mockResolvedValue({ diagramType: 'sequence' });
  mermaid.render.mockResolvedValue({ svg: SVG });
});

afterEach(() => {
  vi.resetAllMocks();
});

describe('StepsView', () => {
  it('shows the first step at load, with no cell marked', async () => {
    const wrapper = await view();

    expect(wrapper.find('.steps-title').text()).toBe('Two candidates in term 6');
    expect(wrapper.find('.steps-label').text()).toBe('t = 0 ms');
    expect(wrapper.find('.steps-body').html()).toContain('<em>times out</em>');
    expect(counter(wrapper)).toBe('1 / 4');
    expect(wrapper.findAll('th').map((th) => th.text())).toEqual(['Server', 'Term', 'Voted for']);
    expect(wrapper.findAll('tbody tr')).toHaveLength(2);
    expect(changed(wrapper)).toEqual([]);
    expect(wrapper.find('.steps-legend').exists()).toBe(false);
    expect(wrapper.get('button[aria-label="Previous step"]').attributes('aria-disabled')).toBe(
      'true',
    );
  });

  it('plays forward and back', async () => {
    const wrapper = await view();

    await next(wrapper);
    expect(counter(wrapper)).toBe('2 / 4');
    expect(wrapper.find('.steps-label').text()).toBe('t = 2 ms');

    await wrapper.get('button[aria-label="Previous step"]').trigger('click');
    expect(counter(wrapper)).toBe('1 / 4');

    // The ends hold: no step before the first, none after the last.
    await wrapper.get('button[aria-label="Previous step"]').trigger('click');
    expect(counter(wrapper)).toBe('1 / 4');
    for (let n = 0; n < 5; n += 1) {
      await next(wrapper);
    }
    expect(counter(wrapper)).toBe('4 / 4');
    expect(wrapper.get('button[aria-label="Next step"]').attributes('aria-disabled')).toBe('true');
  });

  it('plays with the arrow keys when it has focus', async () => {
    const wrapper = await view();
    const figure = wrapper.get('figure.steps');
    expect(figure.attributes('tabindex')).toBe('0');

    await figure.trigger('keydown', { key: 'ArrowRight' });
    expect(counter(wrapper)).toBe('2 / 4');
    await figure.trigger('keydown', { key: 'End' });
    expect(counter(wrapper)).toBe('4 / 4');
    await figure.trigger('keydown', { key: 'ArrowLeft' });
    expect(counter(wrapper)).toBe('3 / 4');
    await figure.trigger('keydown', { key: 'Home' });
    expect(counter(wrapper)).toBe('1 / 4');
    // A key with a modifier belongs to the browser.
    await figure.trigger('keydown', { key: 'ArrowRight', altKey: true });
    expect(counter(wrapper)).toBe('1 / 4');
  });

  it('marks a changed cell by weight, a dot and a text, not by color alone', async () => {
    const wrapper = await view();

    await next(wrapper);

    expect(changed(wrapper)).toEqual(['●6 (changed, was 5)', '●S1 (changed, was empty)']);
    const cell = wrapper.find('td.steps-changed');
    expect(cell.find('.steps-dot').attributes('aria-hidden')).toBe('true');
    expect(cell.find('.sr-only').text()).toBe('(changed, was 5)');
    expect(wrapper.find('.steps-legend').text()).toContain('changed since the previous step');
  });

  it('compares a table with the last table before a diagram, and marks a new row', () => {
    const marks = changes(VOTE.steps, 3).map((row) => row.map((cell) => cell.changed));

    expect(marks).toEqual([
      [false, false, false],
      [false, false, false],
      [true, true, true],
    ]);
    expect(changes(VOTE.steps, 3)[2]![0]!.was).toBeNull();
    expect(changes(VOTE.steps, 2)).toEqual([]);
  });

  it('draws a diagram step with the diagram view', async () => {
    const wrapper = await view();

    await next(wrapper);
    await next(wrapper);

    expect(wrapper.find('table').exists()).toBe(false);
    expect(mermaid.render).toHaveBeenCalledWith(
      expect.any(String),
      'sequenceDiagram\n  S1->>S3: vote?\n',
    );
    expect(wrapper.find('.diagram-svg svg text').text()).toBe('S3');
    expect(wrapper.emitted('broken')).toBeUndefined();
  });

  it('checks every step diagram at load, and names each broken step once', async () => {
    const steps: Steps = {
      ...VOTE,
      steps: [
        ...VOTE.steps,
        { label: 't = 5 ms', html: 'More.', rows: null, diagram: 'graph LR\n  A -->\n' },
      ],
    };
    mermaid.parse.mockImplementation(async (source: string) => {
      if (source.startsWith('graph')) {
        throw new Error('Parse error on line 2');
      }
      return { diagramType: 'sequence' };
    });

    const wrapper = await view(steps);
    // The agent hears of the broken step before the learner plays up to it.
    expect(wrapper.emitted('broken')).toEqual([['s1', 'steps[4].diagram: Parse error on line 2']]);

    await wrapper.get('figure.steps').trigger('keydown', { key: 'End' });
    await flushPromises();

    expect(mermaid.render).not.toHaveBeenCalled();
    expect(wrapper.emitted('broken')).toHaveLength(1);
    expect(wrapper.find('.diagram-broken').exists()).toBe(true);
  });

  it('names every broken step in one report when the step on screen is broken', async () => {
    const broken = (label: string) => ({
      label,
      html: 'More.',
      rows: null,
      diagram: 'graph LR\n  A -->\n',
    });
    const steps: Steps = {
      ...VOTE,
      steps: [broken('t = 0 ms'), ...VOTE.steps, broken('t = 5 ms')],
    };
    mermaid.parse.mockImplementation(async (source: string) => {
      if (source.startsWith('graph')) {
        throw new Error('Parse error on line 2');
      }
      return { diagramType: 'sequence' };
    });

    const wrapper = await view(steps);

    expect(wrapper.find('.diagram-broken').exists()).toBe(true);
    expect(wrapper.emitted('broken')).toEqual([
      ['s1', 'steps[0].diagram: Parse error on line 2\nsteps[5].diagram: Parse error on line 2'],
    ]);
  });

  it('reports a step on screen that parses but does not draw', async () => {
    mermaid.render.mockRejectedValue(new Error('Cannot draw'));

    const wrapper = await view();
    await next(wrapper);
    await next(wrapper);

    expect(wrapper.emitted('broken')).toEqual([['s1', 'steps[2].diagram: Cannot draw']]);
  });

  it('shows every step at once when unrolled, each with its number and its changes', async () => {
    mermaid.render.mockRejectedValue(new Error('Cannot draw'));
    const wrapper = mount(StepsView, { props: { id: 's1', steps: VOTE, unrolled: true } });
    await flushPromises();

    const labels = wrapper.findAll('.steps-step .steps-label').map((label) => label.text());
    expect(labels).toEqual([
      '1 / 4 t = 0 ms',
      '2 / 4 t = 2 ms',
      '3 / 4 t = 3 ms',
      '4 / 4 t = 4 ms',
    ]);
    expect(wrapper.find('button').exists()).toBe(false);
    expect(wrapper.find('figure.steps').attributes('tabindex')).toBeUndefined();
    const second = wrapper.get('.steps-step[data-step="1"]');
    expect(second.findAll('td.steps-changed').map((cell) => cell.text())).toEqual([
      '●6 (changed, was 5)',
      '●S1 (changed, was empty)',
    ]);
    expect(wrapper.find('.steps-step[data-step="2"] .diagram').exists()).toBe(true);
    expect(wrapper.findAll('.steps-legend')).toHaveLength(1);
    // A step diagram that does not draw names its own step.
    expect(wrapper.emitted('broken')).toEqual([['s1', 'steps[2].diagram: Cannot draw']]);
  });

  it('shows the texts of the agent as text', async () => {
    const wrapper = await view({
      title: '<b>Votes</b>',
      columns: ['<i>Server</i>'],
      steps: [{ label: '<u>t</u>', html: 'ok', rows: [['<script>x</script>']], diagram: null }],
    });

    expect(wrapper.find('.steps-title').text()).toBe('<b>Votes</b>');
    expect(wrapper.find('th').text()).toBe('<i>Server</i>');
    expect(wrapper.find('.steps-label').text()).toBe('<u>t</u>');
    expect(wrapper.find('td').text()).toBe('<script>x</script>');
    expect(wrapper.find('script').exists()).toBe(false);
  });
});
