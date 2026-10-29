<script setup lang="ts">
// The blocks of a page, in the order the agent wrote them, then its
// quizzes, each with its debrief, then the choices that the agent asked
// there. Each block takes questions, and its threads show under it. The
// threads about the whole page come at the end, with a button to ask about
// the page, then those whose block a later push took away (section 6 of
// the design).

import { computed, provide, ref, watch } from 'vue';
import type { ChosenAnswer, Page, QuizAnswer } from '@/api/types';
import { WHOLE_PAGE } from '@/place';
import { THREADS, place, type Asking, type ThreadContext } from '@/threads';
import BlocksView from './BlocksView.vue';
import ChoiceView from './ChoiceView.vue';
import CommentBox from './CommentBox.vue';
import QuizView from './QuizView.vue';
import ThreadView from './ThreadView.vue';

const props = withDefaults(
  defineProps<{
    page: Page;
    /** Send a part of the answer to the quiz `n`. Answers whether the server took it. */
    send?: (n: number, answer: QuizAnswer) => Promise<boolean>;
    /** The block whose thread the address names, or WHOLE_PAGE. */
    focus?: string | null;
    /**
     * Send a question about a block, or about the whole page with a null
     * block. Answers whether the server took it.
     */
    ask?: (block: string | null, quote: string | null, body: string) => Promise<boolean>;
    /** Write in a thread. Answers whether the server took it. */
    reply?: (thread: string, body: string) => Promise<boolean>;
    /** Answer a choice. Answers whether the server took it. */
    choose?: (choice: string, answers: ChosenAnswer[]) => Promise<boolean>;
  }>(),
  {
    send: async () => false,
    focus: null,
    ask: async () => false,
    reply: async () => false,
    choose: async () => false,
  },
);

const emit = defineEmits<{
  /** A block of the page, or of a debrief, that the page cannot draw. */
  broken: [page: string, block: string, error: string];
  /** The learner opened the thread of a block, or WHOLE_PAGE, or closed it. */
  focus: [block: string | null];
}>();

const placed = computed(() => place(props.page));
const asking = ref<Asking | null>(null);
/** The Ask box about the whole page is open. */
const askingPage = computed(() => asking.value?.block === null);

const threads: ThreadContext = {
  of: (block) => placed.value.byBlock.get(block) ?? [],
  asking,
  open(block, quote) {
    asking.value = { block, quote };
    emit('focus', block ?? WHOLE_PAGE);
  },
  close() {
    asking.value = null;
  },
  cancel() {
    const open = asking.value;
    asking.value = null;
    if (!open) {
      return;
    }
    // The address would open the box again on a reload. A block with a
    // thread keeps it: the thread is still open under the block. So does
    // the page with a thread about it.
    const held =
      open.block === null ? placed.value.whole.length > 0 : placed.value.byBlock.has(open.block);
    if (!held) {
      emit('focus', null);
    }
  },
  ask: (block, quote, body) => props.ask(block, quote, body),
  reply: (thread, body) => props.reply(thread, body),
};
provide(THREADS, threads);

/** Send the question about the whole page, and close its box once taken. */
async function askPage(body: string): Promise<boolean> {
  if (!(await threads.ask(null, null, body))) {
    return false;
  }
  threads.close();
  return true;
}

// Another page has its own blocks.
watch(
  () => props.page.id,
  () => {
    asking.value = null;
  },
);

// A reload comes back to the thread that the address names. A block with
// no thread yet comes back with its Ask box open, and so does the page.
watch(
  () => [props.page.id, props.focus] as const,
  ([, focus]) => {
    if (focus === WHOLE_PAGE) {
      if (!askingPage.value && !placed.value.whole.length) {
        asking.value = { block: null, quote: null };
      }
      return;
    }
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
    <ChoiceView
      v-for="choice in page.choices"
      :key="choice.id"
      :choice="choice"
      :send="(answers) => choose(choice.id, answers)"
    />
    <section
      v-if="placed.whole.length || askingPage"
      class="threads threads-page"
      aria-label="About this page"
      :data-anchor="WHOLE_PAGE"
    >
      <p class="threads-head">About this page</p>
      <ThreadView
        v-for="thread in placed.whole"
        :key="thread.id"
        :thread="thread"
        :reply="(body) => reply(thread.id, body)"
      />
      <div v-if="askingPage" class="asking" role="group" aria-label="Your question">
        <CommentBox
          label="Your question"
          cancelable
          focus
          :send="askPage"
          @cancel="threads.cancel()"
        />
      </div>
    </section>
    <!-- Before the threads of an earlier version: those stay last. -->
    <p v-if="!askingPage" class="ask-page-end">
      <button
        type="button"
        class="ask-page"
        title="Ask the agent about this page as a whole"
        @click="threads.open(null, null)"
      >
        Ask about this page
      </button>
    </p>
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
