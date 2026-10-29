<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { storeToRefs } from 'pinia';
import AgentPresence from './components/AgentPresence.vue';
import PageView from './components/PageView.vue';
import SyllabusView from './components/SyllabusView.vue';
import type { QuizAnswer } from './api/types';
import { format } from './place';
import { useCourse } from './stores/course';

const store = useCourse();
const {
  course,
  current,
  page,
  focus,
  ready,
  presence,
  status,
  stopped,
  left,
  error,
  ending,
  following,
} = storeToRefs(store);

/** On a narrow screen, the syllabus covers the page until the learner picks one. */
const syllabusOpen = ref(false);

/** The column of the page, which scrolls alone. */
const pane = ref<HTMLElement | null>(null);

// Another page opens at its top, or at the thread that the address names.
// A refresh keeps the id, and the place.
watch(
  () => page.value?.id,
  () => {
    if (!pane.value) {
      return;
    }
    pane.value.scrollTop = 0;
    const block = focus.value;
    if (block) {
      pane.value
        .querySelector(`[data-anchor="${CSS.escape(block)}"]`)
        ?.scrollIntoView({ block: 'start' });
    }
  },
  { flush: 'post' },
);

function openPage(id: string) {
  syllabusOpen.value = false;
  void store.open(id);
}

/** The note of a page that the agent pushed while the learner read another. */
const readyNote = computed(() => {
  if (!ready.value) {
    return '';
  }
  const title = store.titleOf(ready.value);
  return title ? `Page ${ready.value} is ready: ${title}.` : `Page ${ready.value} is ready.`;
});

watch(
  () => course.value?.title,
  (title) => {
    document.title = title ? `${title} · qlearn` : 'qlearn';
  },
  { immediate: true },
);

/** Send a question about a block of the page on the screen. */
function sendQuestion(block: string, quote: string | null, body: string): Promise<boolean> {
  const shown = page.value;
  return shown ? store.ask(shown.id, block, quote, body) : Promise.resolve(false);
}

/** Send a part of the answer to a quiz of the page on the screen. */
function sendAnswer(n: number, answer: QuizAnswer): Promise<boolean> {
  const shown = page.value;
  return shown ? store.answer(shown.id, n, answer) : Promise.resolve(false);
}

function openReady() {
  if (ready.value) {
    void store.open(ready.value);
  }
}

// The browser hands a place back. Nothing is written: the history is
// already on that entry.
function onPop() {
  void store.follow();
}

onMounted(() => {
  void store.listen();
  window.addEventListener('popstate', onPop);
});

onBeforeUnmount(() => {
  window.removeEventListener('popstate', onPop);
});
</script>

<template>
  <div class="shell">
    <header class="top-bar">
      <button
        v-if="course"
        type="button"
        class="syllabus-toggle"
        aria-controls="syllabus"
        :aria-expanded="syllabusOpen"
        @click="syllabusOpen = !syllabusOpen"
      >
        Syllabus
      </button>
      <h1 :title="course?.title">{{ course?.title ?? 'qlearn' }}</h1>
      <span class="bar-end">
        <AgentPresence :presence="presence" :status="status" />
      </span>
    </header>

    <p v-if="stopped" role="alert" class="stopped">
      The qlearn server stopped. Close this tab. To read the course again, run
      <code>qlearn</code> in the terminal.
    </p>
    <p v-else-if="left" role="status" class="agent-left">
      The agent stopped listening. To reattach it, ask it to run <code>qlearn wait</code> again.
    </p>
    <p v-if="ready" role="status" class="ready">
      {{ readyNote }}
      <a :href="format({ page: ready })" @click.prevent="openReady">Read it</a>
    </p>
    <p v-if="error" role="alert" class="error">{{ error }}</p>

    <div class="columns">
      <SyllabusView
        v-if="course"
        id="syllabus"
        :data-open="syllabusOpen"
        :parts="course.parts"
        :shown="page?.id ?? null"
        :current="current"
        @open="openPage"
      />
      <main ref="pane" class="course">
        <template v-if="page">
          <PageView
            :page="page"
            :send="sendAnswer"
            :focus="focus"
            :ask="sendQuestion"
            :reply="store.reply"
            @broken="store.broken"
            @focus="store.openThread"
          />
          <footer class="page-end">
            <p v-if="ending === 'preparing'" role="status" class="preparing">
              The agent prepares the next page<template v-if="status">: {{ status }}</template>
            </p>
            <p v-else-if="ending === 'asked'" role="status" class="asked">
              You asked for the next page.
              <template v-if="presence === 'connected' || presence === 'working'">
                The agent gets the request when it waits again.
              </template>
              <template v-else>
                No agent listens: ask your agent to run <code>qlearn wait</code>.
              </template>
            </p>
            <button
              v-else
              type="button"
              class="next"
              :title="following ? `Open page ${following}` : 'Ask the agent for the next page'"
              @click="store.next()"
            >
              Next
            </button>
          </footer>
        </template>
        <p v-else-if="course && presence === 'working'" role="status" class="preparing empty">
          The agent prepares the first page<template v-if="status">: {{ status }}</template>
        </p>
        <p v-else-if="course" class="empty">
          No page yet. The first page shows here when the agent writes it.
        </p>
      </main>
    </div>
  </div>
</template>
