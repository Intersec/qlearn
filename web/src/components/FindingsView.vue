<script setup lang="ts">
// The findings of the course, at the bottom of the syllabus column
// (section 3 of the design): what the course found in the code that may be
// a bug or a gap, for the learner to check with the team. The agent writes
// them, and the learner edits none. The list opens on demand, so it never
// pushes the syllabus out of sight.

import { computed, ref } from 'vue';
import type { Finding } from '@/api/types';
import { clean } from '@/html';

const props = defineProps<{ findings: Finding[] }>();

const open = ref(false);

const toCheck = computed(() => props.findings.filter((finding) => !finding.checked).length);

// Cleaned once per list, not at every render.
const shown = computed(() =>
  props.findings.map((finding) => ({ ...finding, body: clean(finding.body) })),
);
</script>

<template>
  <section v-if="findings.length" class="findings" aria-label="Findings">
    <button
      type="button"
      class="findings-toggle"
      :aria-expanded="open"
      aria-controls="findings-list"
      @click="open = !open"
    >
      <span class="findings-caret" aria-hidden="true" />
      <span class="findings-name">Findings</span>
      <span class="findings-count">{{ findings.length }}</span>
      <span v-if="toCheck" class="findings-to-check">{{ toCheck }} to check</span>
    </button>
    <ol v-if="open" id="findings-list" class="findings-list">
      <li
        v-for="finding in shown"
        :key="finding.id"
        class="finding"
        :data-finding="finding.id"
        :data-checked="finding.checked"
      >
        <span class="finding-badge">{{ finding.checked ? 'Checked' : 'To check' }}</span>
        <h3 class="finding-title">{{ finding.title }}</h3>
        <!-- eslint-disable-next-line vue/no-v-html -- written by the server, cleaned in html.ts -->
        <div v-if="finding.body" class="md-doc finding-body" v-html="finding.body" />
      </li>
    </ol>
  </section>
</template>
