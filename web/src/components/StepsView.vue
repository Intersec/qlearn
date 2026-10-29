<script setup lang="ts">
// Steps of a page (section 4.5 of the design): a state that changes over
// time, which the learner plays one step at a time. The cheatsheet shows
// every step at once instead: it is read, printed and saved whole, and a
// saved file runs no script to play them.
//
// Every text of the agent goes in as text, except the body of a step: the
// server wrote its HTML, and the page cleans it.

import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { DiagramBlock, Steps } from '@/api/types';
import { Refused, check } from '@/diagram';
import { clean } from '@/html';
import { changes } from '@/steps';
import DiagramView from './DiagramView.vue';
import StepsTable from './StepsTable.vue';

const props = withDefaults(
  defineProps<{
    id: string;
    steps: Steps;
    /** A refused step diagram reaches the agent. */
    reported?: boolean;
    /** Every step at once, one after the other, with no player. */
    unrolled?: boolean;
  }>(),
  { reported: true, unrolled: false },
);

const emit = defineEmits<{
  /** Mermaid refused a step diagram. The error names the step. */
  broken: [block: string, error: string];
}>();

const at = ref(0);
const count = computed(() => props.steps.steps.length);
const step = computed(() => props.steps.steps[at.value] ?? props.steps.steps[0]!);
const body = computed(() => clean(step.value.html));
const cells = computed(() => changes(props.steps.steps, at.value));
const changed = computed(() => cells.value.some((row) => row.some((cell) => cell.changed)));

/** A step diagram, as DiagramView takes it: the block is the steps block. */
function diagramOf(source: string | null): DiagramBlock | null {
  return source === null ? null : { id: props.id, kind: 'diagram', source };
}

const diagram = computed(() => diagramOf(step.value.diagram));

/** Every step, for the unrolled view. */
const all = computed(() =>
  props.steps.steps.map((one, n) => ({
    step: one,
    body: clean(one.html),
    cells: changes(props.steps.steps, n),
    diagram: diagramOf(one.diagram),
  })),
);
const anyChanged = computed(() =>
  all.value.some((one) => one.cells.some((row) => row.some((cell) => cell.changed))),
);

function go(to: number) {
  at.value = Math.min(count.value - 1, Math.max(0, to));
}

const KEYS: Record<string, (from: number) => number> = {
  ArrowLeft: (from) => from - 1,
  ArrowRight: (from) => from + 1,
  Home: () => 0,
  End: () => count.value - 1,
};

function key(event: KeyboardEvent) {
  const to = KEYS[event.key];
  if (!to || event.altKey || event.ctrlKey || event.metaKey) {
    return;
  }
  event.preventDefault();
  go(to(at.value));
}

/** The step in an error, as the server names a field: steps count from 0. */
const where = (n: number) => `steps[${n}].diagram`;

// The agent hears of a broken step once: the server passes on one report
// per block for each push.
let sent = false;
let mounted = true;

function report(error: string) {
  if (sent || !mounted) {
    return;
  }
  sent = true;
  emit('broken', props.id, error);
}

/** Only the newest check reports: a page read again may start one before the last ends. */
let turn = 0;

/**
 * Parse every step diagram at once, so the agent fixes a broken one before
 * the learner plays up to it.
 */
async function checkAll() {
  const mine = (turn += 1);
  const found: string[] = [];
  for (const [n, one] of props.steps.steps.entries()) {
    if (one.diagram === null) {
      continue;
    }
    try {
      await check(one.diagram);
    } catch (error) {
      // A page that cannot load Mermaid says so on the diagram it shows.
      if (!(error instanceof Refused)) {
        return;
      }
      found.push(`${where(n)}: ${error.message}`);
    }
  }
  if (mine === turn && found.length) {
    report(found.join('\n'));
  }
}

/** The check of the steps in progress. */
let checking: Promise<void> = Promise.resolve();

function start() {
  sent = false;
  checking = props.steps.steps.some((one) => one.diagram !== null) ? checkAll() : Promise.resolve();
}

/**
 * Mermaid refused step `n` on screen. The check of every step reports
 * first, as it names them all: this one is heard only if the check found
 * nothing, such as a source that parses but does not draw.
 */
async function refused(n: number, error: string) {
  const name = where(n);
  await checking;
  report(`${name}: ${error}`);
}

// A page read again holds new objects, and may hold fewer steps.
watch(
  () => props.steps,
  () => {
    go(at.value);
    start();
  },
);

onMounted(start);
onBeforeUnmount(() => {
  mounted = false;
});
</script>

<template>
  <figure v-if="unrolled" class="steps steps-unrolled" :aria-label="`${steps.title}, steps`">
    <figcaption class="steps-title">{{ steps.title }}</figcaption>
    <ol class="steps-list">
      <li v-for="(one, n) in all" :key="n" class="steps-step" :data-step="n">
        <p class="steps-label">
          <span class="steps-number">{{ n + 1 }} / {{ count }}</span> {{ one.step.label }}
        </p>
        <!-- eslint-disable-next-line vue/no-v-html -- written by the server, cleaned in html.ts -->
        <p class="md-doc steps-body" v-html="one.body" />
        <StepsTable
          v-if="one.step.rows"
          :columns="steps.columns"
          :rows="one.step.rows"
          :cells="one.cells"
        />
        <DiagramView
          v-if="one.diagram"
          :block="one.diagram"
          :reported="props.reported"
          @broken="(_, error) => refused(n, error)"
        />
      </li>
    </ol>
    <p v-if="anyChanged" class="steps-legend" aria-hidden="true">
      <span class="steps-dot">●</span><strong>Bold</strong>: changed since the previous step
    </p>
  </figure>
  <figure v-else class="steps" tabindex="0" :aria-label="`${steps.title}, steps`" @keydown="key">
    <figcaption class="steps-title">{{ steps.title }}</figcaption>
    <div class="steps-bar">
      <p class="steps-label">{{ step.label }}</p>
      <div class="steps-player">
        <!-- aria-disabled keeps the focus on the button, so the arrows still play. -->
        <button
          type="button"
          class="steps-button"
          aria-label="Previous step"
          :aria-disabled="at === 0"
          @click="go(at - 1)"
        >
          ‹ Previous
        </button>
        <span class="steps-count" aria-live="polite">
          <span aria-hidden="true">{{ at + 1 }} / {{ count }}</span>
          <span class="sr-only">Step {{ at + 1 }} of {{ count }}: {{ step.label }}</span>
        </span>
        <button
          type="button"
          class="steps-button"
          aria-label="Next step"
          :aria-disabled="at === count - 1"
          @click="go(at + 1)"
        >
          Next ›
        </button>
      </div>
    </div>
    <!-- eslint-disable-next-line vue/no-v-html -- written by the server, cleaned in html.ts -->
    <p class="md-doc steps-body" v-html="body" />

    <StepsTable v-if="step.rows" :columns="steps.columns" :rows="step.rows" :cells="cells" />
    <p v-if="changed" class="steps-legend" aria-hidden="true">
      <span class="steps-dot">●</span><strong>Bold</strong>: changed since the previous step
    </p>
    <DiagramView
      v-if="diagram"
      :block="diagram"
      :reported="props.reported"
      @broken="(_, error) => refused(at, error)"
    />
  </figure>
</template>
