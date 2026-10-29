<script setup lang="ts">
// The table of one step. A cell that changed since the previous table is
// bold, with a dot, on the color of a marked line, so a reader who does not
// see the color sees the change too. A screen reader hears it after the
// cell.

import type { Change } from '@/steps';

defineProps<{ columns: string[]; rows: string[][]; cells: Change[][] }>();

/** What a screen reader hears after a changed cell. */
function said(was: string | null): string {
  if (was === null) {
    return '(new)';
  }
  return was === '' ? '(changed, was empty)' : `(changed, was ${was})`;
}
</script>

<template>
  <div class="steps-scroll">
    <table class="steps-table">
      <thead>
        <tr>
          <th v-for="column in columns" :key="column" scope="col">{{ column }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(row, r) in rows" :key="r">
          <td
            v-for="(cell, c) in row"
            :key="c"
            :class="{ 'steps-changed': cells[r]?.[c]?.changed }"
          >
            <template v-if="cells[r]?.[c]?.changed">
              <span class="steps-dot" aria-hidden="true">●</span>{{ cell }}
              <span class="sr-only">{{ said(cells[r]![c]!.was) }}</span>
            </template>
            <template v-else>{{ cell }}</template>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
