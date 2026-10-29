<script setup lang="ts">
// The cheatsheet: the one page that the learner keeps (section 9 of the
// design). The blocks of the agent come first, then the two sections that
// the server builds from the store. The sheet is laid out for A4, in two
// columns, on the screen and in print. It takes no question: it is not a
// page of the course.

import { computed } from 'vue';
import type { Cheatsheet, CodeRange, Course, Grade } from '@/api/types';
import { format } from '@/place';
import BlocksView from './BlocksView.vue';

const props = defineProps<{ cheatsheet: Cheatsheet; course: Course }>();

const emit = defineEmits<{ open: [id: string] }>();

const finished = computed(() => props.course.status === 'finished');

/** Where the store keeps the course, as the learner finds it in the worktree. */
const dir = computed(() => `.learn/${props.course.slug}/`);

/** The name that the server gives the file, so the browser keeps it. */
const markdownName = computed(() => `${props.course.slug}-cheatsheet.md`);

function where(range: CodeRange): string {
  return range.from === range.to
    ? `${range.path}:${range.from}`
    : `${range.path}:${range.from}-${range.to}`;
}

const RESULTS: Record<Grade, string> = { ok: 'Right', partial: 'Partial', missed: 'Missed' };

/** A click with a modifier, or with another button, opens a new tab. */
function onClick(e: MouseEvent, id: string) {
  if (e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || e.button !== 0) {
    return;
  }
  e.preventDefault();
  emit('open', id);
}

function print() {
  window.print();
}
</script>

<template>
  <article class="cheatsheet" aria-label="Cheatsheet">
    <div class="cheatsheet-bar">
      <p v-if="finished" role="note" class="cheatsheet-keep">
        This cheatsheet lives in <code>{{ dir }}</code> of this worktree. Deleting the course
        deletes it, so save it first: print it, pick <strong>Save as PDF</strong> in the print
        dialog, or save it as Markdown for a wiki.
      </p>
      <div class="cheatsheet-exports">
        <button type="button" class="cheatsheet-print" @click="print">Print or save as PDF</button>
        <!-- A link of the same origin carries the cookie of the token. -->
        <a class="cheatsheet-markdown" href="/api/cheatsheet.md" :download="markdownName"
          >Save as Markdown</a
        >
      </div>
    </div>

    <div class="cheatsheet-sheet">
      <!-- No route takes a broken block of the cheatsheet. -->
      <BlocksView :blocks="cheatsheet.blocks" :reported="false" />

      <section class="cheatsheet-section" aria-labelledby="cheatsheet-map">
        <h2 id="cheatsheet-map" class="cheatsheet-head">Map of the code</h2>
        <p v-if="!cheatsheet.map.length" class="cheatsheet-none">The course showed no code.</p>
        <ul v-else class="cheatsheet-list">
          <li
            v-for="range in cheatsheet.map"
            :key="`${range.path} ${range.commit} ${range.from}`"
            class="cheatsheet-range"
          >
            <code class="cheatsheet-where">{{ where(range) }}</code>
            <span v-if="range.repo !== ''" class="cheatsheet-repo">in {{ range.repo }}</span>
            <span class="cheatsheet-commit" :title="range.commit">{{
              range.commit.slice(0, 7)
            }}</span>
            <span class="cheatsheet-lessons">
              <a
                v-for="lesson in range.lessons"
                :key="lesson.id"
                class="cheatsheet-lesson"
                :href="format({ page: lesson.id })"
                @click="onClick($event, lesson.id)"
                ><span class="cheatsheet-id">{{ lesson.id }}</span> {{ lesson.title }}</a
              >
            </span>
          </li>
        </ul>
      </section>

      <section class="cheatsheet-section" aria-labelledby="cheatsheet-revisit">
        <h2 id="cheatsheet-revisit" class="cheatsheet-head">To revisit</h2>
        <p v-if="!cheatsheet.revisit.length" class="cheatsheet-none">Nothing to revisit.</p>
        <ul v-else class="cheatsheet-list">
          <li
            v-for="point in cheatsheet.revisit"
            :key="point.point"
            class="cheatsheet-point"
            :data-result="point.result"
          >
            <span class="cheatsheet-result">{{ RESULTS[point.result] }}</span>
            <span class="cheatsheet-label">{{ point.label }}</span>
            <a
              v-if="point.title !== null"
              class="cheatsheet-lesson"
              :href="format({ page: point.lesson })"
              @click="onClick($event, point.lesson)"
              ><span class="cheatsheet-id">{{ point.lesson }}</span> {{ point.title }}</a
            >
            <!-- A page that the syllabus does not hold has nothing to open. -->
            <span v-else class="cheatsheet-lesson"
              ><span class="cheatsheet-id">{{ point.lesson }}</span></span
            >
          </li>
        </ul>
      </section>
    </div>
  </article>
</template>
