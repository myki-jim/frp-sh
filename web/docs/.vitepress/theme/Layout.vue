<script setup>
import { useData } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { defineAsyncComponent, watchEffect, onMounted } from "vue";
const Landing = defineAsyncComponent(() => import("./RoomLanding.vue"));
const { frontmatter, lang } = useData();
onMounted(() => {
  watchEffect(
    () => {
      if (!frontmatter.value.cinematic)
        document.documentElement.lang = lang.value;
    },
    { flush: "post" },
  );
});
</script>
<template>
  <Landing v-if="frontmatter.cinematic" /><DefaultTheme.Layout v-else />
</template>
