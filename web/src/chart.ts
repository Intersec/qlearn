// The arithmetic of a chart: the range of the y axis, its ticks, and the
// text of a number. ChartView draws with it; it holds no Vue.

import type { Chart } from '@/api/types';

/** The ticks of the y axis: round numbers that cover the values. */
export interface Ticks {
  lo: number;
  hi: number;
  step: number;
  values: number[];
}

/**
 * The y range of a chart, before it is made round. A bar grows from zero,
 * so the range of a bar chart holds zero. A line shows the change, so its
 * range is the one of its values.
 */
export function extent(chart: Chart): [number, number] {
  const values = chart.series.flatMap((one) => one.values).filter((v): v is number => v !== null);
  let lo = Math.min(...values);
  let hi = Math.max(...values);
  if (chart.kind === 'bar') {
    lo = Math.min(lo, 0);
    hi = Math.max(hi, 0);
  }
  if (lo === hi) {
    // Only zeros in a bar chart, or one value in a line: an axis still needs a span.
    const pad = Math.abs(lo) || 1;
    return chart.kind === 'bar' ? [0, 1] : [lo - pad, hi + pad];
  }
  return [lo, hi];
}

/** About `count` ticks at 1, 2 or 5 times a power of ten, from below `lo` to above `hi`. */
export function ticks(lo: number, hi: number, count: number): Ticks {
  const raw = (hi - lo) / Math.max(1, count);
  const power = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * power).find((s) => s >= raw * (1 - 1e-9)) ?? raw;
  const digits = decimals(step);
  // Rounded, so that 0.1 + 0.2 shows as 0.3, and -0 as 0.
  const round = (v: number) => Number(v.toFixed(digits)) + 0;
  // The slack keeps a value that sits on a tick from adding one more.
  const first = Math.floor(lo / step + 1e-9) * step;
  const last = Math.ceil(hi / step - 1e-9) * step;
  const values: number[] = [];
  for (let k = 0; first + k * step <= last + step / 2; k += 1) {
    values.push(round(first + k * step));
  }
  return { lo: values[0]!, hi: values[values.length - 1]!, step, values };
}

/** The decimals that a step needs: 0 for 5, 1 for 0.5, 2 for 0.25. */
function decimals(step: number): number {
  for (let digits = 0; digits < 12; digits += 1) {
    if (Math.abs(Number(step.toFixed(digits)) - step) < step * 1e-9) {
      return digits;
    }
  }
  return 12;
}

/** The text of a tick: 1.5K from a thousand up, else the decimals of the step. */
export function tickText(value: number, step: number): string {
  const size = Math.abs(value);
  if (size < 1000) {
    return value.toFixed(Math.min(decimals(step), 6));
  }
  // The units of the compact text in English: K, M, B, T.
  let unit = 1000;
  while (unit < 1e12 && size >= unit * 1000) {
    unit *= 1000;
  }
  // The step sets the decimals, so that two ticks never share a text. A
  // step too fine for 1.505K takes the whole number.
  const digits = decimals(step / unit);
  if (digits <= 2) {
    return new Intl.NumberFormat('en', {
      notation: 'compact',
      maximumFractionDigits: digits,
    }).format(value);
  }
  const fixed = Math.min(decimals(step), 6);
  return new Intl.NumberFormat('en', {
    minimumFractionDigits: fixed,
    maximumFractionDigits: fixed,
  }).format(value);
}

const FULL = new Intl.NumberFormat('en', { maximumFractionDigits: 6 });

/** A value as the tooltip and the table say it, with its unit. */
export function valueText(value: number | null, unit: string): string {
  if (value === null) {
    return 'no value';
  }
  // Six decimals would show such a value as 0.
  const tiny = value !== 0 && Math.abs(value) < 1e-6;
  const number = tiny ? value.toExponential() : FULL.format(value);
  if (!unit) {
    return number;
  }
  // A percent sign sticks to its number; a word stands apart.
  return unit === '%' ? `${number}%` : `${number} ${unit}`;
}

/** The label of the y axis, with its unit. */
export function yLabel(chart: Chart): string {
  return chart.y.unit ? `${chart.y.label} (${chart.y.unit})` : chart.y.label;
}

/**
 * The path of a bar from the baseline `base` to `top`, with its data end
 * rounded and its foot square, so it reads as growing from the axis.
 */
export function barPath(x: number, width: number, base: number, top: number): string {
  const height = Math.abs(base - top);
  if (height === 0) {
    return '';
  }
  const r = Math.min(4, width / 2, height);
  // Up for a positive value, down for a negative one.
  const dir = top < base ? 1 : -1;
  const end = top + dir * r;
  const sweep = dir === 1 ? 1 : 0;
  return [
    `M${x},${base}`,
    `V${end}`,
    `A${r},${r} 0 0 ${sweep} ${x + r},${top}`,
    `H${x + width - r}`,
    `A${r},${r} 0 0 ${sweep} ${x + width},${end}`,
    `V${base}`,
    'Z',
  ].join('');
}
