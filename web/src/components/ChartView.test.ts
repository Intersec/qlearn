// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import ChartView from './ChartView.vue';
import type { Chart } from '@/api/types';
import { barPath, extent, tickText, ticks, valueText } from '@/chart';

const BAR: Chart = {
  kind: 'bar',
  title: 'Election time, by timeout range',
  x: { label: 'Timeout range (ms)', values: ['150-300', '150-200', '150-151'] },
  y: { label: 'Median election time', unit: 'ms' },
  series: [{ name: '5 servers', values: [283, 410, 1520] }],
};

const LINE: Chart = {
  kind: 'line',
  title: 'The term of each server',
  x: { label: 'Time (s)', values: ['0', '1', '2', '3', '4'] },
  y: { label: 'Term', unit: '' },
  series: [
    { name: 'S1', values: [1, 1, null, 3, 4] },
    { name: 'S2', values: [1, 2, 2, 3, 4] },
  ],
};

function view(chart: Chart) {
  return mount(ChartView, { props: { chart } });
}

/** The y of the end of a bar that is not at its base, from its path. */
function ends(path: string): number[] {
  return [...path.matchAll(/[MVA][^MVAHZ]*/g)].map((part) => {
    const numbers = part[0].slice(1).split(/[ ,]/).map(Number);
    return numbers[numbers.length - 1]!;
  });
}

describe('ChartView', () => {
  it('draws one bar per value, with its color and no legend for one series', () => {
    const wrapper = view(BAR);

    const bars = wrapper.findAll('.chart-bar');
    expect(bars.map((bar) => bar.attributes('data-value'))).toEqual(['283', '410', '1520']);
    expect(bars[0]!.attributes('style')).toContain('var(--series-1)');
    expect(wrapper.find('.chart-legend').exists()).toBe(false);
    expect(wrapper.find('.chart-title').text()).toBe('Election time, by timeout range');
    expect(wrapper.find('.chart-y-label').text()).toBe('Median election time (ms)');
    expect(wrapper.find('.chart-x-label').text()).toBe('Timeout range (ms)');
    expect(wrapper.findAll('.chart-x text').map((text) => text.text())).toEqual([
      '150-300',
      '150-200',
      '150-151',
    ]);
    // Round ticks from zero, a bar grows from it.
    const tickTexts = wrapper.findAll('.chart-tick').map((tick) => tick.text());
    expect(tickTexts).toEqual(['0', '500', '1K', '1.5K', '2K']);
  });

  it('groups the bars of each x value, with a legend for 2 series', () => {
    const wrapper = view({
      ...BAR,
      series: [...BAR.series, { name: '9 servers', values: [301, 455, 1985] }],
    });

    const bars = wrapper.findAll('.chart-bar');
    expect(bars.map((bar) => [bar.attributes('data-series'), bar.attributes('data-x')])).toEqual([
      ['0', '0'],
      ['0', '1'],
      ['0', '2'],
      ['1', '0'],
      ['1', '1'],
      ['1', '2'],
    ]);
    // The two bars of an x value stand side by side, the first series left.
    const left = (at: number) => Number(bars[at]!.attributes('d')!.match(/^M([\d.]+)/)![1]);
    expect(left(0)).toBeLessThan(left(3));
    expect(left(3)).toBeLessThan(left(1));

    const legend = wrapper.findAll('.chart-legend li');
    expect(legend.map((item) => item.text())).toEqual(['5 servers', '9 servers']);
    expect(legend[1]!.find('.chart-key').attributes('style')).toContain('var(--series-2)');
  });

  it('draws a negative bar down from the zero line', () => {
    const wrapper = view({
      ...BAR,
      series: [{ name: 'Change', values: [-40, 0, 120] }],
    });

    const zero = Number(wrapper.find('.chart-zero').attributes('y1'));
    const tickAt = (text: string) =>
      wrapper.findAll('.chart-grid text').find((tick) => tick.text() === text);
    expect(tickAt('0')).toBeDefined();
    expect(Number(tickAt('0')!.attributes('y'))).toBe(zero);

    const bars = wrapper.findAll('.chart-bar');
    const [down, flat, up] = bars.map((bar) => bar.attributes('d') ?? '');
    // Each bar starts at the zero line; the negative one ends below it.
    expect(ends(down!)[0]).toBe(zero);
    expect(Math.max(...ends(down!))).toBeGreaterThan(zero);
    expect(Math.min(...ends(up!))).toBeLessThan(zero);
    // A zero draws no bar, and keeps its place in the table.
    expect(flat).toBe('');
    expect(wrapper.findAll('tbody tr')[1]!.text()).toContain('0 ms');
  });

  it('draws a line per series, broken at a gap, with a legend', () => {
    const wrapper = view(LINE);

    const lines = wrapper.findAll('.chart-line');
    expect(lines).toHaveLength(2);
    expect(lines[0]!.attributes('d')!.match(/M/g)).toHaveLength(2);
    expect(lines[1]!.attributes('d')!.match(/M/g)).toHaveLength(1);
    expect(lines[1]!.attributes('style')).toContain('var(--series-2)');
    expect(wrapper.findAll('.chart-dot[data-series="0"]')).toHaveLength(4);
    expect(wrapper.findAll('.chart-legend li').map((item) => item.text())).toEqual(['S1', 'S2']);
    expect(wrapper.find('.chart-y-label').text()).toBe('Term');
    expect(wrapper.findAll('.chart-bar')).toHaveLength(0);
  });

  it('shows the values of an x value under the pointer', async () => {
    const wrapper = view(LINE);

    await wrapper.findAll('.chart-hit')[2]!.trigger('pointerenter');

    const tip = wrapper.find('.chart-tip');
    expect(tip.find('.chart-tip-head').text()).toBe('Time (s): 2');
    expect(tip.findAll('.chart-tip-row').map((row) => row.text())).toEqual(['S1no value', 'S22']);
    expect(wrapper.find('.chart-cross').exists()).toBe(true);

    await wrapper.find('.chart-plot').trigger('pointerleave');
    expect(wrapper.find('.chart-tip').exists()).toBe(false);
  });

  it('names the chart for a screen reader, and gives its values in a table', () => {
    const wrapper = view(BAR);

    const svg = wrapper.find('svg');
    expect(svg.attributes('role')).toBe('img');
    expect(svg.attributes('aria-label')).toContain('Bar chart: Election time, by timeout range');
    const table = wrapper.find('table');
    expect(table.find('caption').text()).toBe('Election time, by timeout range');
    expect(table.findAll('th[scope="col"]').map((th) => th.text())).toEqual([
      'Timeout range (ms)',
      '5 servers',
    ]);
    expect(table.findAll('tbody tr').map((row) => row.text())).toEqual([
      '150-300283 ms',
      '150-200410 ms',
      '150-1511,520 ms',
    ]);
  });

  it('keeps a turned x label inside the chart, the first one too', () => {
    const labels = Array.from(
      { length: 12 },
      (_, i) => `follower timeout node ${String(i).padStart(2, '0')}`,
    );
    const wrapper = view({
      ...LINE,
      x: { label: 'Setting', values: labels },
      series: [{ name: 'S1', values: labels.map((_, i) => i) }],
    });

    const first = wrapper.find('.chart-x text').attributes('transform')!;
    const x = Number(first.match(/translate\(([\d.]+),/)![1]);
    // A turned label runs left of its x value by its width times cos 45.
    expect(x).toBeGreaterThanOrEqual(labels[0]!.length * 7 * Math.SQRT1_2);
  });

  it('shows the texts of the agent as text, never as HTML', () => {
    const wrapper = view({
      ...BAR,
      title: '<img src=x onerror="window.ran = true">',
      series: [{ name: '<b>bold</b>', values: [1, 2, 3] }],
    });

    expect(wrapper.find('.chart-title').text()).toBe('<img src=x onerror="window.ran = true">');
    expect(wrapper.find('img').exists()).toBe(false);
    expect(wrapper.find('b').exists()).toBe(false);
  });
});

describe('the arithmetic of a chart', () => {
  it('takes round ticks that cover the values', () => {
    expect(ticks(0, 1520, 5).values).toEqual([0, 500, 1000, 1500, 2000]);
    expect(ticks(0.1, 0.7, 5).values).toEqual([0, 0.2, 0.4, 0.6, 0.8]);
    expect(ticks(0.1, 0.7, 6).values).toEqual([0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]);
    expect(ticks(-40, 120, 4).values).toEqual([-50, 0, 50, 100, 150]);
  });

  it('holds zero in the range of a bar chart, and not in the one of a line', () => {
    expect(extent({ ...BAR, series: [{ name: 'a', values: [10, 20, 30] }] })).toEqual([0, 30]);
    expect(extent({ ...LINE, series: [{ name: 'a', values: [10, null, 30, 20, 15] }] })).toEqual([
      10, 30,
    ]);
    expect(extent({ ...BAR, series: [{ name: 'a', values: [0, 0, 0] }] })).toEqual([0, 1]);
  });

  it('writes a number short on an axis and whole in the tooltip', () => {
    expect(tickText(1500, 500)).toBe('1.5K');
    expect(tickText(2_500_000, 500_000)).toBe('2.5M');
    // A fine step in the thousands: the text keeps every tick apart.
    const texts = (lo: number, hi: number) => {
      const y = ticks(lo, hi, 5);
      return y.values.map((value) => tickText(value, y.step));
    };
    expect(texts(1500, 1520)).toEqual(['1,500', '1,505', '1,510', '1,515', '1,520']);
    expect(new Set(texts(1000, 1010)).size).toBe(texts(1000, 1010).length);
    expect(texts(10000, 10002)).toContain('10,000.5');
    expect(tickText(0.25, 0.05)).toBe('0.25');
    expect(valueText(1985.5, 'ms')).toBe('1,985.5 ms');
    expect(valueText(45, '%')).toBe('45%');
    expect(valueText(null, 'ms')).toBe('no value');
    // A value too small for six decimals keeps its digits.
    expect(valueText(0.00000015, 's')).toBe('1.5e-7 s');
  });

  it('rounds the data end of a bar and keeps its foot square', () => {
    expect(barPath(10, 20, 100, 40)).toBe('M10,100V44A4,4 0 0 1 14,40H26A4,4 0 0 1 30,44V100Z');
    expect(barPath(10, 20, 100, 102)).toBe('M10,100V100A2,2 0 0 0 12,102H28A2,2 0 0 0 30,100V100Z');
    expect(barPath(10, 20, 100, 100)).toBe('');
  });
});
