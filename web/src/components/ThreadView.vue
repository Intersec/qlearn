<script setup lang="ts">
// A thread: the quote of the learner, the comments, and a box to reply.

import { computed } from 'vue';
import type { Thread } from '@/api/types';
import { clean } from '@/html';
import CommentBox from './CommentBox.vue';

const props = defineProps<{
  thread: Thread;
  /** Write in the thread. Answers whether the server took it. */
  reply: (body: string) => Promise<boolean>;
}>();

// Cleaned once per thread, not at every render.
const comments = computed(() =>
  props.thread.comments.map((comment) => ({ ...comment, body: clean(comment.body) })),
);

/** The learner wrote last: the agent owes the next comment. */
const waiting = computed(() => props.thread.comments.at(-1)?.author === 'learner');
</script>

<template>
  <section class="thread" :data-thread="thread.id" :aria-label="`Thread ${thread.id}`">
    <blockquote v-if="thread.quote" class="thread-quote">{{ thread.quote }}</blockquote>
    <article
      v-for="(comment, at) in comments"
      :key="at"
      class="thread-comment"
      :data-author="comment.author"
    >
      <p class="thread-who">{{ comment.author === 'agent' ? 'Agent' : 'You' }}</p>
      <!-- eslint-disable-next-line vue/no-v-html -- written by the server, cleaned in html.ts -->
      <div class="md-doc thread-body" v-html="comment.body" />
    </article>
    <p v-if="waiting" class="thread-waiting" role="status">The agent answers here.</p>
    <CommentBox label="Reply" :send="reply" />
  </section>
</template>
