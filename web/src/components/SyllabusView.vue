<script setup lang="ts">
// The syllabus in the left column: the parts and their pages, each with
// its state and its score (section 3 of the design). A planned page has no
// content yet, so it is shown but cannot be opened. The choices that name
// no page come first, in the slot (section 7).

import { computed } from 'vue';
import type { PageRef, PageState, Part, Score } from '@/api/types';
import { format } from '@/place';

const props = defineProps<{
  parts: Part[];
  /** The page on the screen. */
  shown: string | null;
  /** The page that the agent pushed last. */
  current: string | null;
}>();

const emit = defineEmits<{ open: [id: string] }>();

/** One part with no title is the implicit part of a course: a flat list. */
const flat = computed(() => props.parts.length === 1 && props.parts[0].title === '');

const empty = computed(() => props.parts.every((part) => part.pages.length === 0));

const STATES: Record<PageState, string> = {
  planned: 'Planned',
  taught: 'Taught',
  answered: 'Answered',
  debriefed: 'Debriefed',
};

/** `right` comes in steps of 0.5: a partial point counts half. */
function scoreOf(score: Score): string {
  const right = Number.isInteger(score.right) ? String(score.right) : score.right.toFixed(1);
  return `${right}/${score.total}`;
}

/** A click with a modifier, or with another button, opens a new tab. */
function onClick(e: MouseEvent, id: string) {
  if (e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || e.button !== 0) {
    return;
  }
  e.preventDefault();
  emit('open', id);
}

function hint(page: PageRef): string {
  const parts = [STATES[page.state]];
  if (page.score) {
    parts.push(`${scoreOf(page.score)} right`);
  }
  if (page.id === props.current) {
    parts.push('the page that the agent wrote last');
  }
  if (page.state === 'planned') {
    parts.push('the agent has not written it yet');
  }
  return parts.join(', ');
}
</script>

<template>
  <nav class="syllabus" aria-label="Syllabus">
    <slot />
    <p v-if="empty" class="syllabus-empty">The syllabus shows here when the agent writes it.</p>
    <section v-for="(part, at) in parts" v-else :key="at" class="syllabus-part">
      <h2 v-if="!flat && part.title" class="syllabus-part-title">{{ part.title }}</h2>
      <ol class="syllabus-pages">
        <li
          v-for="page in part.pages"
          :key="page.id"
          class="syllabus-page"
          :data-page="page.id"
          :data-state="page.state"
          :data-shown="page.id === shown || undefined"
          :data-current="page.id === current || undefined"
          :title="hint(page)"
        >
          <span v-if="page.state === 'planned'" class="syllabus-entry" aria-disabled="true">
            <span class="syllabus-mark" aria-hidden="true" />
            <span class="syllabus-id">{{ page.id }}</span>
            <span class="syllabus-title">{{ page.title }}</span>
            <span class="sr-only">({{ STATES[page.state] }})</span>
          </span>
          <a
            v-else
            class="syllabus-entry"
            :href="format({ page: page.id })"
            :aria-current="page.id === shown ? 'page' : undefined"
            @click="onClick($event, page.id)"
          >
            <span class="syllabus-mark" aria-hidden="true" />
            <span class="syllabus-id">{{ page.id }}</span>
            <span class="syllabus-title">{{ page.title }}</span>
            <span class="sr-only">({{ STATES[page.state] }})</span>
            <span v-if="page.id === current" class="syllabus-latest">latest</span>
            <span v-if="page.score" class="syllabus-score">{{ scoreOf(page.score) }}</span>
          </a>
        </li>
      </ol>
    </section>
  </nav>
</template>
