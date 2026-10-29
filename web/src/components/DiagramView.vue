<script setup lang="ts">
// A diagram, drawn in the page: the server cannot run Mermaid. A source
// that Mermaid refuses shows its error in place of the diagram, and goes
// back to the agent, which fixes the page (section 4.3 of the design). The
// cheatsheet sends nothing back, so its diagrams promise no fix.

import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { DiagramBlock } from '@/api/types';
import { Refused, WIDE, dark, draw, naturalWidth, watchTheme } from '@/diagram';

const props = withDefaults(
  defineProps<{
    block: DiagramBlock;
    /** A refused source reaches the agent. */
    reported?: boolean;
  }>(),
  { reported: true },
);

const emit = defineEmits<{
  /** Mermaid refused the source of the block. */
  broken: [block: string, error: string];
}>();

const svg = ref<string | null>(null);
/**
 * Too wide for one column of the cheatsheet: there it spans both, so that
 * its text stays readable.
 */
const wide = computed(() => svg.value !== null && (naturalWidth(svg.value) ?? 0) > WIDE);
/**
 * The theme of the drawing. Print takes the light theme, and Mermaid
 * writes its colors into the SVG, so print lightens a dark drawing.
 */
const scheme = ref<'light' | 'dark'>('light');
/** Why the diagram is not drawn, and whether the agent hears of it. */
const failure = ref<{ message: string; refused: boolean } | null>(null);

/** Only the newest drawing lands: a theme change may start one before the last ends. */
let turn = 0;
let mounted = true;

async function render() {
  if (!mounted) {
    return;
  }
  const mine = (turn += 1);
  const { id, source } = props.block;
  const drawnIn = dark() ? 'dark' : 'light';
  try {
    const drawn = await draw(source);
    if (mounted && mine === turn) {
      svg.value = drawn;
      scheme.value = drawnIn;
      failure.value = null;
    }
  } catch (error) {
    if (!mounted || mine !== turn) {
      return;
    }
    const message = error instanceof Error ? error.message : String(error);
    const refused = error instanceof Refused;
    svg.value = null;
    failure.value = { message, refused };
    if (refused) {
      emit('broken', id, message);
    }
  }
}

// A page read again holds new block objects: the page may then report a
// source that it reported for an older version.
watch(() => props.block, render);

let stop = () => {};
onMounted(() => {
  void render();
  stop = watchTheme(() => void render());
});
onBeforeUnmount(() => {
  mounted = false;
  stop();
});
</script>

<template>
  <figure class="diagram" :data-scheme="scheme" :data-wide="wide ? 'true' : undefined">
    <!-- eslint-disable-next-line vue/no-v-html -- drawn by Mermaid in strict mode, cleaned in diagram.ts -->
    <div v-if="svg !== null" class="diagram-svg" v-html="svg" />
    <div v-else-if="failure" class="diagram-broken">
      <p class="diagram-broken-head">
        <template v-if="failure.refused && reported">
          Mermaid cannot draw this diagram. The agent gets the error, and fixes the page.
        </template>
        <template v-else-if="failure.refused">Mermaid cannot draw this diagram.</template>
        <template v-else>The page cannot draw diagrams. Reload it to try again.</template>
      </p>
      <pre class="diagram-error">{{ failure.message }}</pre>
      <pre class="diagram-source"><code>{{ block.source }}</code></pre>
    </div>
    <p v-else class="diagram-drawing">Drawing the diagram…</p>
  </figure>
</template>
