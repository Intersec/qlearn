<script setup lang="ts">
// An image of the repository, as the server read it at the push. The
// caption says where it comes from, as the header of an excerpt does.
//
// An <img> runs no script of an SVG and loads nothing for it, so the page
// never puts the SVG itself in the page.

import { computed } from 'vue';
import type { ImageBlock } from '@/api/types';

const props = defineProps<{ image: ImageBlock }>();

const source = computed(() => `/api/media/${props.image.media}`);

// As short as `git log --oneline` prints it. The title holds the whole hash.
const short = computed(() => props.image.commit.slice(0, 7));
</script>

<template>
  <figure class="image">
    <figcaption class="image-head">
      <span class="image-path">{{ image.path }}</span>
      <span v-if="image.repo !== ''" class="image-repo">in {{ image.repo }}</span>
      <span class="image-commit" :title="image.commit">{{ short }}</span>
    </figcaption>
    <div class="image-body">
      <!-- The full size opens in a tab of its own. -->
      <a :href="source" target="_blank" rel="noopener noreferrer" title="Open at full size">
        <img
          :src="source"
          :alt="image.alt"
          :width="image.width ?? undefined"
          :height="image.height ?? undefined"
          decoding="async"
        />
      </a>
    </div>
  </figure>
</template>
