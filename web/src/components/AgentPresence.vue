<script setup lang="ts">
// The agent in the top bar: is it there, does it write, and what it says
// it does (`qlearn status`).

import type { Presence } from '@/stores/course';

defineProps<{ presence: Presence; status: string | null }>();

const LABELS: Record<Presence, string> = {
  absent: 'No agent',
  connected: 'Agent connected',
  working: 'Agent working',
  left: 'Agent left',
};

const TITLES: Record<Presence, string> = {
  absent: 'No agent came yet. Ask an agent to teach this course.',
  connected: 'An agent waits for your requests.',
  working: 'The agent works on your request.',
  left: 'The agent stopped listening. Ask it to run qlearn wait again.',
};
</script>

<template>
  <span class="presence" role="status">
    <span class="agent-pill" :data-presence="presence" :title="TITLES[presence]">
      {{ LABELS[presence] }}
    </span>
    <span v-if="status" class="agent-status" :title="status">{{ status }}</span>
  </span>
</template>
