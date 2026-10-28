<script setup lang="ts">
// A quiz under its lesson, in its first state (section 5.2 of the design).
//
// The open question comes alone, so the options of the choices cannot give
// its answer away. A quiz with no open question starts at its choices.
// Sending an answer comes with the routes of the learner, so Send waits.

import { computed, ref } from 'vue';
import type { Quiz } from '@/api/types';
import { clean } from '@/html';

const props = defineProps<{ quiz: Quiz; n: number }>();

/** What the learner typed, kept while the quiz stays on the screen. */
const answer = ref('');
/** The option that the learner picked for each choice. */
const picked = ref<(number | null)[]>([]);

// Cleaned once per quiz, not at every render.
const choices = computed(() =>
  props.quiz.choices.map((choice) => ({ ...choice, options: choice.options.map(clean) })),
);

const LATER = 'This release of qlearn cannot send an answer yet.';
</script>

<template>
  <section class="quiz" :data-quiz="n" :aria-label="`Quiz ${n}`">
    <p class="quiz-head">Quiz {{ n }}</p>
    <form v-if="quiz.open" class="quiz-open" @submit.prevent>
      <label :for="`quiz-${n}-open`" class="quiz-prompt">{{ quiz.open.prompt }}</label>
      <textarea
        :id="`quiz-${n}-open`"
        v-model="answer"
        class="quiz-text"
        rows="4"
        placeholder="Your answer"
      />
      <p class="quiz-end">
        <button type="submit" class="quiz-send" disabled>Send</button>
        <span class="quiz-note">{{ LATER }}</span>
      </p>
    </form>
    <form v-else class="quiz-choices" @submit.prevent>
      <fieldset v-for="(choice, at) in choices" :key="at" class="quiz-choice">
        <legend class="quiz-legend">
          <span class="quiz-header">{{ choice.header }}</span>
          <span class="quiz-prompt">{{ choice.prompt }}</span>
        </legend>
        <label v-for="(option, k) in choice.options" :key="k" class="quiz-option">
          <input v-model="picked[at]" type="radio" :name="`quiz-${n}-${at}`" :value="k" />
          <!-- eslint-disable-next-line vue/no-v-html -- written by the server, cleaned in html.ts -->
          <span class="md-doc quiz-option-text" v-html="option" />
        </label>
      </fieldset>
      <p class="quiz-end">
        <button type="submit" class="quiz-send" disabled>Send</button>
        <span class="quiz-note">{{ LATER }}</span>
      </p>
    </form>
  </section>
</template>
