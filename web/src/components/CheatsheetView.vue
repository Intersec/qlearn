<script setup lang="ts">
// The cheatsheet: the one page that the learner keeps (section 9 of the
// design). The blocks of the agent come first, then the two sections that
// the server builds from the store. The sheet is laid out for A4, in two
// columns, on the screen and in print. It takes no question: it is not a
// page of the course.

import { computed, ref } from 'vue';
import type { Cheatsheet, CodeRange, Course, Grade } from '@/api/types';
import { format } from '@/place';
import { fileName, sheetFile } from '@/sheetfile';
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

const sheet = ref<HTMLElement | null>(null);
/** The page builds the HTML file: it draws the diagrams again and reads the images. */
const saving = ref(false);
const failed = ref<string | null>(null);

async function saveHtml() {
  if (!sheet.value || saving.value) {
    return;
  }
  saving.value = true;
  failed.value = null;
  try {
    const html = await sheetFile(
      sheet.value,
      props.cheatsheet,
      `Cheatsheet: ${props.course.title}`,
    );
    download(html, fileName(props.course.slug));
  } catch (error) {
    failed.value = error instanceof Error ? error.message : String(error);
  } finally {
    saving.value = false;
  }
}

function download(html: string, name: string) {
  const url = URL.createObjectURL(new Blob([html], { type: 'text/html;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  // The browser reads the blob after the click returns.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
</script>

<template>
  <article class="cheatsheet" aria-label="Cheatsheet">
    <div class="cheatsheet-bar">
      <p v-if="finished" role="note" class="cheatsheet-keep">
        This cheatsheet lives in <code>{{ dir }}</code> of this worktree. Deleting the course
        deletes it, so save it first: print it or pick <strong>Save as PDF</strong> in the print
        dialog, save it as one HTML file, or as Markdown for a wiki.
      </p>
      <div class="cheatsheet-exports">
        <button type="button" class="cheatsheet-print" @click="print">Print or save as PDF</button>
        <button
          type="button"
          class="cheatsheet-html"
          :aria-disabled="saving"
          :aria-busy="saving"
          @click="saveHtml"
        >
          Save as HTML
        </button>
        <!-- A link of the same origin carries the cookie of the token. -->
        <a class="cheatsheet-markdown" href="/api/cheatsheet.md" :download="markdownName"
          >Save as Markdown</a
        >
      </div>
      <p v-if="failed" role="alert" class="cheatsheet-failed">
        The page cannot build the HTML file: {{ failed }}
      </p>
    </div>

    <div ref="sheet" class="cheatsheet-sheet">
      <!-- No route takes a broken block of the cheatsheet. -->
      <BlocksView :blocks="cheatsheet.blocks" :reported="false" unrolled />

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
