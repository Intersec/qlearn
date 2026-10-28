<script setup lang="ts">
// The blocks of a page, in the order the agent wrote them.

import { computed } from 'vue';
import type { Page } from '@/api/types';
import { clean } from '@/html';
import CalloutView from './CalloutView.vue';
import DiagramView from './DiagramView.vue';
import ExcerptView from './ExcerptView.vue';

const props = defineProps<{ page: Page }>();

const emit = defineEmits<{
  /** A block of the page that the page cannot draw. */
  broken: [page: string, block: string, error: string];
}>();

// Cleaned once per page, not at every render.
const blocks = computed(() =>
  props.page.blocks.map((block) =>
    block.kind === 'text' ? { ...block, html: clean(block.html) } : block,
  ),
);
</script>

<template>
  <article class="lesson">
    <template v-for="block in blocks" :key="block.id">
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
        <DiagramView :block="block" @broken="(id, error) => emit('broken', page.id, id, error)" />
      </section>
    </template>
  </article>
</template>
