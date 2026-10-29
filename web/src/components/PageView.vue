<script setup lang="ts">
// The blocks of a page, in the order the agent wrote them, then its
// quizzes, each with its debrief. Each block takes questions, and its
// threads show under it. The threads about the whole page come at the end,
// then those whose block a later push took away (section 6 of the design).

import { computed, provide, ref, watch } from 'vue';
import type { Page, QuizAnswer } from '@/api/types';
import { THREADS, place, type Asking } from '@/threads';
import BlocksView from './BlocksView.vue';
import QuizView from './QuizView.vue';
import ThreadView from './ThreadView.vue';

const props = withDefaults(
  defineProps<{
    page: Page;
    /** Send a part of the answer to the quiz `n`. Answers whether the server took it. */
    send?: (n: number, answer: QuizAnswer) => Promise<boolean>;
    /** The block whose thread the address names. */
    focus?: string | null;
    /** Send a question about a block. Answers whether the server took it. */
    ask?: (block: string, quote: string | null, body: string) => Promise<boolean>;
    /** Write in a thread. Answers whether the server took it. */
    reply?: (thread: string, body: string) => Promise<boolean>;
  }>(),
  {
    send: async () => false,
    focus: null,
    ask: async () => false,
    reply: async () => false,
  },
);

const emit = defineEmits<{
  /** A block of the page, or of a debrief, that the page cannot draw. */
  broken: [page: string, block: string, error: string];
  /** The learner opened the thread of a block, or closed it. */
  focus: [block: string | null];
}>();

const placed = computed(() => place(props.page));
const asking = ref<Asking | null>(null);

provide(THREADS, {
  of: (block) => placed.value.byBlock.get(block) ?? [],
  asking,
  open(block, quote) {
    asking.value = { block, quote };
    emit('focus', block);
  },
  close() {
    asking.value = null;
  },
  cancel() {
    const block = asking.value?.block;
    asking.value = null;
    // The address would open the box again on a reload. A block with a
    // thread keeps it: the thread is still open under the block.
    if (block && !placed.value.byBlock.has(block)) {
      emit('focus', null);
    }
  },
  ask: (block, quote, body) => props.ask(block, quote, body),
  reply: (thread, body) => props.reply(thread, body),
});

// Another page has its own blocks.
watch(
  () => props.page.id,
  () => {
    asking.value = null;
  },
);

// A reload comes back to the thread that the address names. A block with
// no thread yet comes back with its Ask box open.
watch(
  () => [props.page.id, props.focus] as const,
  ([, focus]) => {
    if (!focus || asking.value?.block === focus || placed.value.byBlock.has(focus)) {
      return;
    }
    const held =
      props.page.blocks.some((block) => block.id === focus) ||
      props.page.quizzes.some((quiz) => quiz.debrief?.blocks.some((block) => block.id === focus));
    if (held) {
      asking.value = { block: focus, quote: null };
    }
  },
  { immediate: true },
);
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
    <section v-if="placed.whole.length" class="threads threads-page" aria-label="About this page">
      <p class="threads-head">About this page</p>
      <ThreadView
        v-for="thread in placed.whole"
        :key="thread.id"
        :thread="thread"
        :reply="(body) => reply(thread.id, body)"
      />
    </section>
    <section
      v-if="placed.earlier.length"
      class="threads threads-earlier"
      aria-label="On an earlier version of this page"
    >
      <p class="threads-head">On an earlier version of this page</p>
      <ThreadView
        v-for="thread in placed.earlier"
        :key="thread.id"
        :thread="thread"
        :reply="(body) => reply(thread.id, body)"
      />
    </section>
  </article>
</template>
