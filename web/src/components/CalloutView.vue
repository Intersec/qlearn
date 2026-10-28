<script setup lang="ts">
// A callout: a box with its marker, so the learner tells a hook from a
// pitfall at a glance.

import { computed } from 'vue';
import type { Marker } from '@/api/types';
import { clean } from '@/html';

const props = defineProps<{ marker: Marker; html: string }>();

const MARKERS: Record<Marker, { sign: string; name: string }> = {
  hook: { sign: '🧠', name: 'Hook' },
  teaser: { sign: '🔭', name: 'Teaser' },
  pitfall: { sign: '⚠️', name: 'Pitfall' },
  note: { sign: 'ℹ️', name: 'Note' },
};

const shown = computed(() => MARKERS[props.marker]);

// Cleaned once per block, not at every render.
const body = computed(() => clean(props.html));
</script>

<template>
  <aside class="callout" :data-marker="marker">
    <p class="callout-head">
      <span class="callout-sign" aria-hidden="true">{{ shown.sign }}</span>
      <span class="callout-name">{{ shown.name }}</span>
    </p>
    <!-- eslint-disable-next-line vue/no-v-html -- written by the server, cleaned in html.ts -->
    <div class="md-doc callout-body" v-html="body" />
  </aside>
</template>
