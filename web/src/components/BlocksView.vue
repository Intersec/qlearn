<script setup lang="ts">
// Blocks in the order the agent wrote them: those of a lesson, or those of
// the debrief of a quiz, which is a page source too (section 5.4 of the
// design). Each block takes questions: an Ask button, and its threads
// under it (section 6).

import { computed, inject } from 'vue';
import type { Block } from '@/api/types';
import { clean } from '@/html';
import { THREADS, quoteIn } from '@/threads';
import CalloutView from './CalloutView.vue';
import ChartView from './ChartView.vue';
import CommentBox from './CommentBox.vue';
import DiagramView from './DiagramView.vue';
import ExcerptView from './ExcerptView.vue';
import ImageView from './ImageView.vue';
import StepsView from './StepsView.vue';
import ThreadView from './ThreadView.vue';

const props = withDefaults(
  defineProps<{
    blocks: Block[];
    /** A broken block reaches the agent: false in the cheatsheet. */
    reported?: boolean;
  }>(),
  { reported: true },
);

const emit = defineEmits<{
  /** A block that the page cannot draw. */
  broken: [block: string, error: string];
}>();

/** None outside a page: the blocks then take no question. */
const threads = inject(THREADS, null);

// Cleaned once per set of blocks, not at every render.
const shown = computed(() =>
  props.blocks.map((block) =>
    block.kind === 'text' ? { ...block, html: clean(block.html) } : block,
  ),
);

/** The block element of an Ask button. */
function blockOf(event: Event): Element | null {
  const anchor = (event.currentTarget as Element | null)?.closest('.thread-anchor');
  return anchor?.querySelector(':scope > [data-block]') ?? null;
}

/**
 * The quote taken when the button is pressed. A click may clear the
 * selection before its handler runs, so the press reads it first.
 */
let pressed: { block: string; quote: string | null } | null = null;

function press(block: string, event: Event) {
  pressed = { block, quote: quoteIn(blockOf(event)) };
}

function ask(block: string, event: Event) {
  // A key press on the button comes with no mouse press.
  const quote = pressed?.block === block ? pressed.quote : quoteIn(blockOf(event));
  pressed = null;
  threads?.open(block, quote);
}

async function send(block: string, quote: string | null, body: string): Promise<boolean> {
  if (!threads || !(await threads.ask(block, quote, body))) {
    return false;
  }
  threads.close();
  return true;
}
</script>

<template>
  <div v-for="block in shown" :key="block.id" class="thread-anchor" :data-anchor="block.id">
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
    <section v-else-if="block.kind === 'chart'" class="block" :data-block="block.id">
      <ChartView :chart="block.chart" />
    </section>
    <section v-else-if="block.kind === 'image'" class="block" :data-block="block.id">
      <ImageView :image="block" />
    </section>
    <section v-else-if="block.kind === 'steps'" class="block" :data-block="block.id">
      <StepsView
        :id="block.id"
        :steps="block.steps"
        :reported="reported"
        @broken="(id, error) => emit('broken', id, error)"
      />
    </section>
    <section v-else class="block" :data-block="block.id">
      <DiagramView
        :block="block"
        :reported="reported"
        @broken="(id, error) => emit('broken', id, error)"
      />
    </section>

    <template v-if="threads">
      <!-- The press keeps the selection: it is the quote of the question. -->
      <button
        type="button"
        class="ask"
        title="Ask the agent about this block. Select text in it first to quote it."
        @mousedown.prevent="press(block.id, $event)"
        @click="ask(block.id, $event)"
      >
        Ask
      </button>
      <div
        v-if="threads.of(block.id).length || threads.asking.value?.block === block.id"
        class="threads"
      >
        <ThreadView
          v-for="thread in threads.of(block.id)"
          :key="thread.id"
          :thread="thread"
          :reply="(body) => threads!.reply(thread.id, body)"
        />
        <div
          v-if="threads.asking.value?.block === block.id"
          class="asking"
          role="group"
          aria-label="Your question"
        >
          <blockquote v-if="threads.asking.value.quote" class="thread-quote">
            {{ threads.asking.value.quote }}
          </blockquote>
          <CommentBox
            label="Your question"
            cancelable
            focus
            :send="(body) => send(block.id, threads!.asking.value?.quote ?? null, body)"
            @cancel="threads.cancel()"
          />
        </div>
      </div>
    </template>
  </div>
</template>
