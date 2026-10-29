<script setup lang="ts">
// A box to write a comment in: a question under a block, or a reply in a
// thread. What the learner typed stays until the server takes it.

import { onMounted, ref } from 'vue';

const props = defineProps<{
  /** The name of the box, for a reader of the screen and as a placeholder. */
  label: string;
  /** Send the body. Answers whether the server took it. */
  send: (body: string) => Promise<boolean>;
  /** The box offers Cancel. */
  cancelable?: boolean;
  /** The learner opened the box: the keyboard goes into it. */
  focus?: boolean;
}>();

const emit = defineEmits<{ cancel: [] }>();

const body = ref('');
/** A body on its way: a second press sends nothing. */
const sending = ref(false);
const area = ref<HTMLTextAreaElement | null>(null);

onMounted(() => {
  if (props.focus) {
    area.value?.focus();
  }
});

async function save() {
  if (sending.value || body.value.trim() === '') {
    return;
  }
  sending.value = true;
  try {
    if (await props.send(body.value)) {
      body.value = '';
    }
  } finally {
    sending.value = false;
  }
}

// Ctrl+Enter sends, as in most tools where one writes a comment.
function onKey(event: KeyboardEvent) {
  if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    void save();
  } else if (event.key === 'Escape' && props.cancelable) {
    emit('cancel');
  }
}
</script>

<template>
  <form class="comment-box" @submit.prevent="save">
    <textarea
      ref="area"
      v-model="body"
      class="comment-text"
      rows="2"
      :aria-label="label"
      :placeholder="label"
      @keydown="onKey"
    />
    <p class="comment-end">
      <button type="submit" class="comment-send" :disabled="body.trim() === '' || sending">
        Send
      </button>
      <button v-if="cancelable" type="button" class="comment-cancel" @click="emit('cancel')">
        Cancel
      </button>
    </p>
  </form>
</template>
