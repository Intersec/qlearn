<script setup lang="ts">
// An excerpt: lines of the repository, as the server read them at the push.
// The header says where they come from, so the learner can open the file.

import { computed } from 'vue';
import type { Excerpt } from '@/api/types';
import { pieces } from '@/code';

const props = defineProps<{ excerpt: Excerpt }>();

const where = computed(() => {
  const { path, from, to } = props.excerpt;
  return from === to ? `${path}:${from}` : `${path}:${from}-${to}`;
});

// As short as `git log --oneline` prints it. The title holds the whole hash.
const short = computed(() => props.excerpt.commit.slice(0, 7));

const rows = computed(() =>
  props.excerpt.lines.map((line, at) => {
    const number = props.excerpt.from + at;
    return {
      number,
      marked: props.excerpt.marked.includes(number),
      // null: the hidden line, which the server keeps until the answer.
      pieces: line === null ? null : pieces(line),
    };
  }),
);
</script>

<template>
  <figure class="excerpt">
    <figcaption class="excerpt-head">
      <span class="excerpt-path">{{ where }}</span>
      <span v-if="excerpt.repo !== ''" class="excerpt-repo">in {{ excerpt.repo }}</span>
      <span class="excerpt-commit" :title="excerpt.commit">{{ short }}</span>
    </figcaption>
    <div class="excerpt-body">
      <table class="excerpt-code">
        <tbody>
          <tr
            v-for="row in rows"
            :key="row.number"
            :class="{ 'excerpt-marked': row.marked }"
            :data-line="row.number"
          >
            <td class="excerpt-number">{{ row.number }}</td>
            <td class="excerpt-text">
              <span
                v-if="row.pieces === null"
                class="excerpt-hidden"
                role="img"
                aria-label="A hidden line"
                title="A hidden line"
              />
              <template v-else>
                <!-- An empty line stays empty: its number keeps the row one
                     line high. -->
                <span v-for="(piece, p) in row.pieces" :key="p" :class="piece.cls">{{
                  piece.text
                }}</span>
              </template>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </figure>
</template>
