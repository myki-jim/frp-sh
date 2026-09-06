<script setup>
import { useData } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { defineAsyncComponent, watchEffect, onMounted } from "vue";
const Landing = defineAsyncComponent(() => import("./Landing.vue"));
const { frontmatter, lang } = useData();
onMounted(() => {
  watchEffect(
    () => {
      document.documentElement.lang = frontmatter.value.cinematic
        ? "en"
        : lang.value;
    },
    { flush: "post" },
  );
});
</script>
<template>
  <Landing v-if="frontmatter.cinematic" /><DefaultTheme.Layout v-else />
</template>
