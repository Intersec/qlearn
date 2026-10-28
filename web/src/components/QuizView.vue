<script setup lang="ts">
// A quiz under its lesson, in the steps of section 5.2 of the design.
//
// 1. The open question comes alone, so the options of the choices cannot
//    give its answer away. A quiz with no open question starts at step 2.
// 2. The choices, each with an "Other" box. A quiz with no choices sends
//    its answer at step 1.
// 3. The server grades the choices, and the page draws right and wrong.
//    The agent writes the debrief.
//
// The page never decides whether an answer is right: the right option
// comes from the server, once the learner has sent the choices.

import { computed, ref } from 'vue';
import type { Choice, GradedChoice, Quiz, QuizAnswer } from '@/api/types';
import { clean } from '@/html';

const props = defineProps<{
  quiz: Quiz;
  n: number;
  /** Send a part of the answer. Answers whether the server took it. */
  send: (answer: QuizAnswer) => Promise<boolean>;
}>();

/** What the learner typed, kept while the quiz stays on the screen. */
const text = ref('');
/** The option that the learner picked for each choice. */
const picked = ref<(number | null)[]>(props.quiz.choices.map(() => null));
/** The text of the "Other" box of each choice. */
const others = ref<string[]>(props.quiz.choices.map(() => ''));
/** A part on its way to the server: a second press sends nothing. */
const sending = ref(false);

type Step = 'open' | 'choices' | 'graded';

const step = computed<Step>(() => {
  if (props.quiz.open && props.quiz.open.sent === null) {
    return 'open';
  }
  if (props.quiz.choices.some((choice) => choice.graded === null)) {
    return 'choices';
  }
  return 'graded';
});

// Cleaned once per quiz, not at every render.
const choices = computed(() =>
  props.quiz.choices.map((choice) => ({ ...choice, options: choice.options.map(clean) })),
);

const graded = computed(() =>
  choices.value.flatMap((choice) => (choice.graded ? [{ ...choice, graded: choice.graded }] : [])),
);

const allPicked = computed(() => picked.value.every((option) => option !== null));

async function sendPart(answer: QuizAnswer) {
  if (sending.value) {
    return;
  }
  sending.value = true;
  try {
    await props.send(answer);
  } finally {
    sending.value = false;
  }
}

function sendOpen() {
  if (text.value.trim() !== '') {
    void sendPart({ open: text.value });
  }
}

function sendChoices() {
  if (!allPicked.value) {
    return;
  }
  void sendPart({
    choices: picked.value.map((option, at) => ({
      option: option ?? 0,
      other: others.value[at].trim() === '' ? null : others.value[at],
    })),
  });
}

/** How an option of a graded choice is drawn. */
function mark(graded: GradedChoice, k: number): 'right' | 'wrong' | 'none' {
  if (k === graded.answer) {
    return 'right';
  }
  return k === graded.picked ? 'wrong' : 'none';
}

/** What a reader of the screen hears after an option of a graded choice. */
function said(graded: GradedChoice, k: number): string {
  const parts = [];
  if (k === graded.picked) {
    parts.push('your pick');
  }
  if (k === graded.answer) {
    parts.push('the right option');
  }
  return parts.length ? `(${parts.join(', ')})` : '';
}

function verdict(choice: Choice & { graded: GradedChoice }): string {
  return choice.graded.ok ? 'Right' : 'Wrong';
}
</script>

<template>
  <section class="quiz" :data-quiz="n" :data-step="step" :aria-label="`Quiz ${n}`">
    <p class="quiz-head">Quiz {{ n }}</p>

    <form v-if="step === 'open' && quiz.open" class="quiz-open" @submit.prevent="sendOpen">
      <label :for="`quiz-${n}-open`" class="quiz-prompt">{{ quiz.open.prompt }}</label>
      <textarea
        :id="`quiz-${n}-open`"
        v-model="text"
        class="quiz-text"
        rows="4"
        placeholder="Your answer"
      />
      <p class="quiz-end">
        <button type="submit" class="quiz-send" :disabled="text.trim() === '' || sending">
          Send
        </button>
        <span v-if="quiz.choices.length" class="quiz-note"
          >The choices come after this answer.</span
        >
      </p>
    </form>
    <div v-else-if="quiz.open" class="quiz-sent">
      <p class="quiz-prompt">{{ quiz.open.prompt }}</p>
      <p class="quiz-sent-text">{{ quiz.open.sent }}</p>
    </div>

    <form v-if="step === 'choices'" class="quiz-choices" @submit.prevent="sendChoices">
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
        <label class="quiz-other">
          <span class="quiz-other-label">Other</span>
          <input
            v-model="others[at]"
            type="text"
            class="quiz-other-text"
            placeholder="Anything to add, in your own words"
          />
        </label>
      </fieldset>
      <p class="quiz-end">
        <button type="submit" class="quiz-send" :disabled="!allPicked || sending">Send</button>
        <span v-if="!allPicked" class="quiz-note">Pick one option in each question.</span>
      </p>
    </form>

    <template v-if="step === 'graded'">
      <section
        v-for="(choice, at) in graded"
        :key="at"
        class="quiz-choice quiz-graded"
        :data-ok="choice.graded.ok"
      >
        <p class="quiz-legend">
          <span class="quiz-header">{{ choice.header }}</span>
          <span class="quiz-verdict">{{ verdict(choice) }}</span>
          <span class="quiz-prompt">{{ choice.prompt }}</span>
        </p>
        <ul class="quiz-options">
          <li
            v-for="(option, k) in choice.options"
            :key="k"
            class="quiz-option"
            :data-mark="mark(choice.graded, k)"
            :data-picked="k === choice.graded.picked || undefined"
          >
            <span class="quiz-sign" aria-hidden="true">{{
              mark(choice.graded, k) === 'right'
                ? '✓'
                : mark(choice.graded, k) === 'wrong'
                  ? '✗'
                  : ''
            }}</span>
            <!-- eslint-disable-next-line vue/no-v-html -- written by the server, cleaned in html.ts -->
            <span class="md-doc quiz-option-text" v-html="option" />
            <span v-if="k === choice.graded.picked" class="quiz-tag" aria-hidden="true"
              >Your pick</span
            >
            <span v-if="said(choice.graded, k)" class="sr-only">{{ said(choice.graded, k) }}</span>
          </li>
        </ul>
        <p v-if="choice.graded.other" class="quiz-other-sent">
          <span class="quiz-other-label">Other</span>
          <span>{{ choice.graded.other }}</span>
        </p>
      </section>
      <p class="quiz-debrief" role="status">The agent writes the debrief.</p>
    </template>
  </section>
</template>
