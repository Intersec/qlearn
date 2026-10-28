<script setup lang="ts">
// The blocks of a page, in the order the agent wrote them.

import { computed } from 'vue';
import type { Page } from '@/api/types';
import { clean } from '@/html';

const props = defineProps<{ page: Page }>();

// Cleaned once per page, not at every render.
const blocks = computed(() =>
  props.page.blocks.map((block) => ({ id: block.id, html: clean(block.html) })),
);
</script>

<template>
  <article class="lesson">
    <!-- eslint-disable vue/no-v-html -- written by the server, cleaned in html.ts -->
    <section
      v-for="block in blocks"
      :key="block.id"
      class="md-doc block"
      :data-block="block.id"
      v-html="block.html"
    />
    <!-- eslint-enable vue/no-v-html -->
  </article>
</template>
