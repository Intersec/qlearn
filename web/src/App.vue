<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { storeToRefs } from 'pinia';
import AgentPresence from './components/AgentPresence.vue';
import CheatsheetView from './components/CheatsheetView.vue';
import ChoiceView from './components/ChoiceView.vue';
import FindingsView from './components/FindingsView.vue';
import PageView from './components/PageView.vue';
import SyllabusView from './components/SyllabusView.vue';
import type { QuizAnswer } from './api/types';
import { CHEATSHEET, format } from './place';
import { useCourse } from './stores/course';

const store = useCourse();
const {
  course,
  current,
  page,
  focus,
  cheatsheet,
  sheetShown,
  ready,
  presence,
  status,
  stopped,
  left,
  error,
  ending,
  following,
  choices,
  choicesError,
  choosing,
} = storeToRefs(store);

/** On a narrow screen, the syllabus covers the page until the learner picks one. */
/** An agent waits on the server, so a request of the learner reaches it. */
const listening = computed(() => presence.value === 'connected' || presence.value === 'working');

const syllabusOpen = ref(false);

/** The column of the page, which scrolls alone. */
const pane = ref<HTMLElement | null>(null);

// Another page, or the cheatsheet, opens at its top, and a page at the
// thread that the address names. A refresh keeps the id, and the place.
// The page under the cheatsheet keeps its id, so the sheet is watched too.
watch(
  [() => page.value?.id, sheetShown],
  ([, shown]) => {
    if (!pane.value) {
      return;
    }
    pane.value.scrollTop = 0;
    const block = focus.value;
    if (!shown && block) {
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

function openCheatsheet() {
  syllabusOpen.value = false;
  void store.openCheatsheet();
}

/** The course is over and its cheatsheet waits for the learner. */
const sheetReady = computed(
  () =>
    course.value?.status === 'finished' && cheatsheet.value?.written === true && !sheetShown.value,
);

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

/** Send a question about a block of the page on the screen, or about the page. */
function sendQuestion(block: string | null, quote: string | null, body: string): Promise<boolean> {
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
        :data-choosing="choosing || undefined"
        @click="syllabusOpen = !syllabusOpen"
      >
        Syllabus
        <span v-if="choosing" class="syllabus-toggle-asks">
          <span aria-hidden="true">•</span><span class="sr-only">: the agent asks you</span>
        </span>
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
    <p v-if="sheetReady" role="status" class="ready sheet-ready">
      Cheatsheet ready: the course on one sheet, to keep.
      <a :href="CHEATSHEET" @click.prevent="openCheatsheet">Open it</a>
    </p>
    <p v-if="error" role="alert" class="error">{{ error }}</p>
    <p v-if="choicesError" role="alert" class="error">
      The choices of the agent cannot show. {{ choicesError }}
    </p>

    <div class="columns">
      <SyllabusView
        v-if="course"
        id="syllabus"
        :data-open="syllabusOpen"
        :parts="course.parts"
        :shown="sheetShown ? null : (page?.id ?? null)"
        :current="current"
        :cheatsheet="cheatsheet?.written ?? false"
        :sheet-shown="sheetShown"
        @open="openPage"
        @cheatsheet="openCheatsheet"
      >
        <!-- The open ones first: an answer stays below, read only. -->
        <ChoiceView
          v-for="choice in [...choices].sort(
            (a, b) => Number(a.answers !== null) - Number(b.answers !== null),
          )"
          :key="choice.id"
          :choice="choice"
          compact
          :send="(answers) => store.choose(choice.id, answers)"
        />
        <template #end>
          <FindingsView :findings="course.findings" />
        </template>
      </SyllabusView>
      <main ref="pane" class="course">
        <CheatsheetView
          v-if="sheetShown && cheatsheet && course"
          :cheatsheet="cheatsheet"
          :course="course"
          @open="openPage"
        />
        <template v-else-if="page">
          <PageView
            :page="page"
            :send="sendAnswer"
            :focus="focus"
            :ask="sendQuestion"
            :reply="store.reply"
            :choose="store.choose"
            @broken="store.broken"
            @focus="store.openThread"
          />
          <footer class="page-end">
            <p
              v-if="ending === 'debriefing'"
              role="status"
              :class="listening ? 'preparing' : 'asked'"
            >
              The next page comes after the debrief of your quiz.
              <template v-if="!listening">
                No agent listens: ask your agent to run <code>qlearn wait</code>.
              </template>
            </p>
            <p v-else-if="ending === 'preparing'" role="status" class="preparing">
              The agent prepares the next page<template v-if="status">: {{ status }}</template>
            </p>
            <p v-else-if="ending === 'asked'" role="status" class="asked">
              You asked for the next page.
              <template v-if="listening">
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
        <p v-else-if="course && choosing" class="empty">
          The agent asks you to choose, in the syllabus column.
        </p>
        <p v-else-if="course" class="empty">
          No page yet. The first page shows here when the agent writes it.
        </p>
      </main>
    </div>
  </div>
</template>
