<script setup lang="ts">
// Blocks in the order the agent wrote them: those of a lesson, or those of
// the debrief of a quiz, which is a page source too (section 5.4 of the
// design).

import { computed } from 'vue';
import type { Block } from '@/api/types';
import { clean } from '@/html';
import CalloutView from './CalloutView.vue';
import DiagramView from './DiagramView.vue';
import ExcerptView from './ExcerptView.vue';

const props = defineProps<{ blocks: Block[] }>();

const emit = defineEmits<{
  /** A block that the page cannot draw. */
  broken: [block: string, error: string];
}>();

// Cleaned once per set of blocks, not at every render.
const shown = computed(() =>
  props.blocks.map((block) =>
    block.kind === 'text' ? { ...block, html: clean(block.html) } : block,
  ),
);
</script>

<template>
  <template v-for="block in shown" :key="block.id">
    <!-- eslint-disable vue/no-v-html -- written by the server, cleaned in html.ts -->
    <section
      v-if="block.kind === 'text'"
      class="md-doc block"
      :data-block="block.id"
      v-html="block.html"
    />
    <!-- eslint-enable vue/no-v-html -->
    <section v-else-if="block.kind === 'callout'" class="block" :data-block="block.id">
      <CalloutView :marker="block.marker" :html="block.html" />
    </section>
    <section v-else-if="block.kind === 'excerpt'" class="block" :data-block="block.id">
      <ExcerptView :excerpt="block.excerpt" />
    </section>
    <section v-else class="block" :data-block="block.id">
      <DiagramView :block="block" @broken="(id, error) => emit('broken', id, error)" />
    </section>
  </template>
</template>
