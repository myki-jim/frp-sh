<script setup>
defineProps({ name: { type: String, default: "arrow" } });
</script>
<template>
  <svg
    class="site-icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <template v-if="name === 'sun'">
      <circle cx="12" cy="12" r="4" />
      <path
        d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5"
      />
    </template>
    <path
      v-else-if="name === 'moon'"
      d="M20.6 13.2A8.7 8.7 0 0 1 10.8 3.4a8.7 8.7 0 1 0 9.8 9.8Z"
    />
    <path v-else d="M5 19 19 5M5 5h14v14" />
  </svg>
</template>
