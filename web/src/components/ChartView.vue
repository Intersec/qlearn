<script setup lang="ts">
// A chart of a page, drawn in plain SVG (section 4.4 of the design). The
// server checked the format, so every chart that comes here can be drawn.
// The colors are tokens of the page, so a theme change needs no redraw.
//
// Every text of the agent goes in as text, never as HTML: Vue escapes it.

import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { Chart } from '@/api/types';
import { barPath, extent, tickText, ticks, valueText, yLabel } from '@/chart';

const props = defineProps<{ chart: Chart }>();

/** The width before the first measure, and in a test, which lays out nothing. */
const width = ref(640);
const box = ref<HTMLElement | null>(null);

let observer: ResizeObserver | null = null;
onMounted(() => {
  if (!box.value || typeof ResizeObserver === 'undefined') {
    return;
  }
  observer = new ResizeObserver(([entry]) => {
    const measured = Math.floor(entry?.contentRect.width ?? 0);
    if (measured > 0) {
      width.value = measured;
    }
  });
  observer.observe(box.value);
});
onBeforeUnmount(() => observer?.disconnect());

/** The width of a character of the axis text, 12px, as an estimate. */
const CHAR = 7;
/** The widest and the narrowest bar, and the gap between two bars. */
const BAR = 24;
const GAP = 2;

const kind = computed(() => (props.chart.kind === 'bar' ? 'Bar chart' : 'Line chart'));

const layout = computed(() => {
  const { chart } = props;
  const count = chart.x.values.length;
  const plotHeight = width.value < 480 ? 200 : 260;
  const y = ticks(...extent(chart), Math.max(3, Math.round(plotHeight / 50)));
  const tickTexts = y.values.map((value) => tickText(value, y.step));

  const right = 12;
  const tickLeft = Math.max(...tickTexts.map((text) => text.length)) * CHAR + 12;
  const bandOf = (left: number) => Math.max(40, width.value - left - right) / count;

  // A label wider than its band turns, and a turned label taller than its
  // band shows on every n-th band only.
  const longest = Math.max(...chart.x.values.map((value) => value.length)) * CHAR;
  const turned = longest > bandOf(tickLeft) - 6;
  // A turned label runs left of the center of its band. On a narrow screen
  // the first one passes the ticks, so the margin grows until it fits:
  // left + band / 2 >= reach, where the band shrinks as left grows.
  const reach = longest * Math.SQRT1_2 + 8;
  const half = 1 / (2 * count);
  const left = turned
    ? Math.max(tickLeft, Math.ceil((reach - (width.value - right) * half) / (1 - half)))
    : tickLeft;
  const plotWidth = Math.max(40, width.value - left - right);
  const band = plotWidth / count;
  const every = turned ? Math.ceil(16 / band) : 1;
  const bottom = turned ? Math.ceil(longest * Math.SQRT1_2) + 16 : 24;
  const top = 10;

  const scale = (value: number) => top + ((y.hi - value) / (y.hi - y.lo)) * plotHeight;
  const center = (i: number) => left + band * (i + 0.5);

  return {
    height: top + plotHeight + bottom,
    top,
    left,
    plotWidth,
    plotHeight,
    band,
    turned,
    every,
    scale,
    center,
    ticks: y.values.map((value, i) => ({ value, y: scale(value), text: tickTexts[i]! })),
    zero: y.lo <= 0 && y.hi >= 0 ? scale(0) : null,
    base: scale(Math.min(Math.max(0, y.lo), y.hi)),
  };
});

const colors = computed(() => props.chart.series.map((_, i) => `var(--series-${i + 1})`));

/** The bars of a bar chart, grouped by x value, each series at its place. */
const bars = computed(() => {
  if (props.chart.kind !== 'bar') {
    return [];
  }
  const { band, left, scale, base } = layout.value;
  const series = props.chart.series;
  const n = series.length;
  const room = band * 0.8 - GAP * (n - 1);
  const thick = Math.max(1, Math.min(BAR, room / n));
  const group = thick * n + GAP * (n - 1);
  return series.flatMap((one, s) =>
    one.values.map((value, i) => {
      const x = left + band * i + (band - group) / 2 + s * (thick + GAP);
      return {
        key: `${s}-${i}`,
        series: s,
        x: i,
        value,
        path: value === null ? '' : barPath(x, thick, base, scale(value)),
      };
    }),
  );
});

/** The lines of a line chart, broken at each gap, and their points. */
const lines = computed(() => {
  if (props.chart.kind !== 'line') {
    return [];
  }
  const { scale, center } = layout.value;
  const count = props.chart.x.values.length;
  // Points on a long line are noise; a point alone between gaps must show.
  const dotted = count <= 24;
  return props.chart.series.map((one, s) => {
    let path = '';
    const dots: { x: number; y: number; i: number }[] = [];
    one.values.forEach((value, i) => {
      if (value === null) {
        return;
      }
      const x = center(i);
      const y = scale(value);
      const before = one.values[i - 1] ?? null;
      path += `${before === null ? 'M' : 'L'}${x},${y}`;
      const alone = before === null && (one.values[i + 1] ?? null) === null;
      if (dotted || alone) {
        dots.push({ x, y, i });
      }
    });
    return { series: s, path, dots };
  });
});

/** The x value under the pointer, or null. */
const active = ref<number | null>(null);

const tip = computed(() => {
  const i = active.value;
  if (i === null) {
    return null;
  }
  const { center, plotWidth, left } = layout.value;
  // The tip stays inside the chart, at either side of its x value.
  const x = center(i);
  const leftSide = x > left + plotWidth / 2;
  return {
    x,
    leftSide,
    head: `${props.chart.x.label}: ${props.chart.x.values[i]}`,
    rows: props.chart.series.map((one, s) => ({
      name: one.name,
      color: colors.value[s]!,
      text: valueText(one.values[i] ?? null, props.chart.y.unit),
    })),
  };
});
</script>

<template>
  <figure ref="box" class="chart" :data-kind="chart.kind">
    <figcaption class="chart-title">{{ chart.title }}</figcaption>
    <ul v-if="chart.series.length > 1" class="chart-legend" aria-hidden="true">
      <li v-for="(one, s) in chart.series" :key="one.name">
        <span class="chart-key" :data-kind="chart.kind" :style="{ background: colors[s] }" />
        {{ one.name }}
      </li>
    </ul>
    <p class="chart-y-label" aria-hidden="true">{{ yLabel(chart) }}</p>
    <div class="chart-plot" @pointerleave="active = null">
      <svg
        role="img"
        :aria-label="`${kind}: ${chart.title}. ${chart.x.label} by ${yLabel(chart)}. The table after it gives the values.`"
        :width="width"
        :height="layout.height"
        :viewBox="`0 0 ${width} ${layout.height}`"
      >
        <g class="chart-grid">
          <template v-for="tick in layout.ticks" :key="tick.value">
            <line
              :x1="layout.left"
              :x2="layout.left + layout.plotWidth"
              :y1="tick.y"
              :y2="tick.y"
            />
            <text class="chart-tick" :x="layout.left - 8" :y="tick.y" dy="0.32em" text-anchor="end">
              {{ tick.text }}
            </text>
          </template>
        </g>
        <rect
          v-if="active !== null && chart.kind === 'bar'"
          class="chart-band"
          :x="layout.left + layout.band * active"
          :y="layout.top"
          :width="layout.band"
          :height="layout.plotHeight"
        />
        <line
          v-if="layout.zero !== null"
          class="chart-zero"
          :x1="layout.left"
          :x2="layout.left + layout.plotWidth"
          :y1="layout.zero"
          :y2="layout.zero"
        />
        <g class="chart-x">
          <template v-for="(value, i) in chart.x.values" :key="value">
            <text
              v-if="i % layout.every === 0"
              :transform="
                layout.turned
                  ? `translate(${layout.center(i)},${layout.top + layout.plotHeight + 10}) rotate(-45)`
                  : undefined
              "
              :x="layout.turned ? 0 : layout.center(i)"
              :y="layout.turned ? 0 : layout.top + layout.plotHeight + 16"
              :text-anchor="layout.turned ? 'end' : 'middle'"
              :dy="layout.turned ? '0.32em' : undefined"
            >
              {{ value }}
            </text>
          </template>
        </g>
        <template v-if="chart.kind === 'bar'">
          <path
            v-for="bar in bars"
            v-show="bar.path"
            :key="bar.key"
            class="chart-bar"
            :data-series="bar.series"
            :data-x="bar.x"
            :data-value="bar.value"
            :d="bar.path"
            :style="{ fill: colors[bar.series] }"
          />
        </template>
        <template v-else>
          <line
            v-if="active !== null"
            class="chart-cross"
            :x1="layout.center(active)"
            :x2="layout.center(active)"
            :y1="layout.top"
            :y2="layout.top + layout.plotHeight"
          />
          <g v-for="line in lines" :key="line.series">
            <path
              class="chart-line"
              :data-series="line.series"
              :d="line.path"
              :style="{ stroke: colors[line.series] }"
            />
            <circle
              v-for="dot in line.dots"
              :key="dot.i"
              class="chart-dot"
              :data-series="line.series"
              :cx="dot.x"
              :cy="dot.y"
              :r="active === dot.i ? 5 : 4"
              :style="{ fill: colors[line.series] }"
            />
          </g>
        </template>
        <!-- One target per x value, over the marks: the pointer finds a
             thin bar or a line, and a touch shows the tip too. -->
        <rect
          v-for="(value, i) in chart.x.values"
          :key="`hit-${value}`"
          class="chart-hit"
          :x="layout.left + layout.band * i"
          :y="layout.top"
          :width="layout.band"
          :height="layout.plotHeight"
          @pointerenter="active = i"
          @pointerdown="active = i"
        />
      </svg>
      <div
        v-if="tip"
        class="chart-tip"
        :class="{ 'chart-tip-left': tip.leftSide }"
        :style="{ left: `${tip.x}px` }"
        aria-hidden="true"
      >
        <p class="chart-tip-head">{{ tip.head }}</p>
        <p v-for="row in tip.rows" :key="row.name" class="chart-tip-row">
          <span class="chart-key" :data-kind="chart.kind" :style="{ background: row.color }" />
          <span class="chart-tip-name">{{ row.name }}</span>
          <span class="chart-tip-value">{{ row.text }}</span>
        </p>
      </div>
    </div>
    <p class="chart-x-label" aria-hidden="true">{{ chart.x.label }}</p>
    <!-- The values for a screen reader, which reads no SVG mark. -->
    <table class="sr-only">
      <caption>
        {{
          chart.title
        }}
      </caption>
      <thead>
        <tr>
          <th scope="col">{{ chart.x.label }}</th>
          <th v-for="one in chart.series" :key="one.name" scope="col">{{ one.name }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(value, i) in chart.x.values" :key="value">
          <th scope="row">{{ value }}</th>
          <td v-for="one in chart.series" :key="one.name">
            {{ valueText(one.values[i] ?? null, chart.y.unit) }}
          </td>
        </tr>
      </tbody>
    </table>
  </figure>
</template>
