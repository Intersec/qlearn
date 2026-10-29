<script setup lang="ts">
// A choice that the agent asks, in place of a dialog of its terminal
// (section 7 of the design): at the end of its page, or at the top of the
// syllabus column. Each question takes radio buttons, or checkboxes when
// it takes several picks, and free text. Once sent, the answer shows read
// only. A choice is never graded, so nothing here says right or wrong.

import { computed, ref } from 'vue';
import type { ChosenAnswer, Choosing } from '@/api/types';

const props = withDefaults(
  defineProps<{
    choice: Choosing;
    /** Send the answers. Answers whether the server took them. */
    send: (answers: ChosenAnswer[]) => Promise<boolean>;
    /** In the syllabus column, which is narrow. */
    compact?: boolean;
  }>(),
  { compact: false },
);

/** The pick of each question that takes one. */
const one = ref<(number | null)[]>(props.choice.questions.map(() => null));
/** The picks of each question that takes several. */
const many = ref<number[][]>(props.choice.questions.map(() => []));
/** The free text of each question. */
const others = ref<string[]>(props.choice.questions.map(() => ''));
/** The answer on its way to the server: a second press sends nothing. */
const sending = ref(false);

function picks(at: number): number[] {
  if (props.choice.questions[at].multi) {
    return [...many.value[at]].sort((a, b) => a - b);
  }
  const picked = one.value[at];
  return picked === null ? [] : [picked];
}

/** Each question has a pick, or words of the learner. */
const ready = computed(() =>
  props.choice.questions.every((_, at) => picks(at).length > 0 || others.value[at].trim() !== ''),
);

async function submit() {
  if (!ready.value || sending.value) {
    return;
  }
  sending.value = true;
  try {
    await props.send(
      props.choice.questions.map((_, at) => ({
        picked: picks(at),
        other: others.value[at].trim() === '' ? null : others.value[at].trim(),
      })),
    );
  } finally {
    sending.value = false;
  }
}

/** The answered questions, each with the labels of its picks. */
const answered = computed(() => {
  const answers = props.choice.answers;
  if (!answers) {
    return null;
  }
  return props.choice.questions.map((question, at) => ({
    header: question.header,
    prompt: question.prompt,
    labels: (answers[at]?.picked ?? []).map((k) => question.options[k]?.label ?? `${k}`),
    other: answers[at]?.other ?? null,
  }));
});
</script>

<template>
  <section
    class="choice"
    :data-choice="choice.id"
    :data-answered="answered !== null"
    :data-compact="compact || undefined"
    :aria-label="answered ? 'Your choice' : 'The agent asks you to choose'"
  >
    <p class="choice-head">{{ answered ? 'Your choice' : 'The agent asks' }}</p>

    <form v-if="!answered" class="choice-form" @submit.prevent="submit">
      <fieldset v-for="(question, at) in choice.questions" :key="at" class="choice-question">
        <legend class="choice-legend">
          <span class="choice-header">{{ question.header }}</span>
          <span class="choice-prompt">{{ question.prompt }}</span>
        </legend>
        <label v-for="(option, k) in question.options" :key="k" class="choice-option">
          <input
            v-if="question.multi"
            v-model="many[at]"
            type="checkbox"
            :name="`${choice.id}-${at}`"
            :value="k"
          />
          <input v-else v-model="one[at]" type="radio" :name="`${choice.id}-${at}`" :value="k" />
          <span class="choice-option-text">
            <span class="choice-label">{{ option.label }}</span>
            <span v-if="option.description" class="choice-description">{{
              option.description
            }}</span>
          </span>
        </label>
        <label class="choice-other">
          <span class="choice-other-label">Other</span>
          <input
            v-model="others[at]"
            type="text"
            class="choice-other-text"
            placeholder="Or answer in your own words"
          />
        </label>
      </fieldset>
      <p class="choice-end">
        <button type="submit" class="choice-send" :disabled="!ready || sending">Send</button>
        <span v-if="!ready" class="choice-note">
          {{
            choice.questions.length > 1
              ? 'Answer each question: pick an option, or write your own.'
              : 'Pick an option, or write your own.'
          }}
        </span>
      </p>
    </form>

    <template v-else>
      <div v-for="(question, at) in answered" :key="at" class="choice-question choice-sent">
        <p class="choice-legend">
          <span class="choice-header">{{ question.header }}</span>
          <span class="choice-prompt">{{ question.prompt }}</span>
        </p>
        <ul v-if="question.labels.length" class="choice-picked">
          <li v-for="label in question.labels" :key="label" class="choice-pick">
            <span class="choice-sign" aria-hidden="true">✓</span>
            <span>{{ label }}</span>
          </li>
        </ul>
        <p v-if="question.other" class="choice-other-sent">
          <span class="choice-other-label">Other</span>
          <span>{{ question.other }}</span>
        </p>
      </div>
    </template>
  </section>
</template>
