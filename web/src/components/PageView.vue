<script setup lang="ts">
// The blocks of a page, in the order the agent wrote them, then its
// quizzes, each with its debrief.

import type { Page, QuizAnswer } from '@/api/types';
import BlocksView from './BlocksView.vue';
import QuizView from './QuizView.vue';

withDefaults(
  defineProps<{
    page: Page;
    /** Send a part of the answer to the quiz `n`. Answers whether the server took it. */
    send?: (n: number, answer: QuizAnswer) => Promise<boolean>;
  }>(),
  { send: async () => false },
);

const emit = defineEmits<{
  /** A block of the page, or of a debrief, that the page cannot draw. */
  broken: [page: string, block: string, error: string];
}>();
</script>

<template>
  <article class="lesson">
    <BlocksView :blocks="page.blocks" @broken="(id, error) => emit('broken', page.id, id, error)" />
    <!-- A push of the page keeps its quizzes, so the rank names a quiz in
         its page. The page id keeps a draft on its own page. -->
    <QuizView
      v-for="(quiz, at) in page.quizzes"
      :key="`${page.id}-${at}`"
      :quiz="quiz"
      :n="at + 1"
      :send="(answer) => send(at + 1, answer)"
      @broken="(id, error) => emit('broken', page.id, id, error)"
    />
  </article>
</template>
