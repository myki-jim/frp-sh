<script setup>
import { useData, useRoute } from "vitepress";
import DefaultTheme from "vitepress/theme";
import {
  defineAsyncComponent,
  watchEffect,
  onMounted,
  watch,
  nextTick,
  onBeforeUnmount,
  ref,
} from "vue";
import DocsControls from "./DocsControls.vue";
import { mountDocsMotion } from "./docs-motion.js";
const shell = ref(null);
const route = useRoute();
let disposeMotion,
  stopMotion,
  generation = 0;
const Landing = defineAsyncComponent(() => import("./RoomLanding.vue"));
const { frontmatter, lang } = useData();
onMounted(() => {
  stopMotion = watch(
    () => [route.path, frontmatter.value.cinematic],
    async () => {
      const current = ++generation;
      disposeMotion?.();
      disposeMotion = undefined;
      await nextTick();
      if (current === generation && shell.value)
        disposeMotion = mountDocsMotion(shell.value);
    },
    { immediate: true, flush: "post" },
  );
  watchEffect(
    () => {
      if (!frontmatter.value.cinematic)
        document.documentElement.lang = lang.value;
    },
    { flush: "post" },
  );
});
onBeforeUnmount(() => {
  generation++;
  stopMotion?.();
  disposeMotion?.();
});
</script>
<template>
  <Landing v-if="frontmatter.cinematic" />
  <div v-else ref="shell" class="doc-shell">
    <div class="docs-reading-progress" aria-hidden="true"></div>
    <DefaultTheme.Layout>
      <template #nav-bar-content-after><DocsControls /></template>
      <template #doc-before
        ><p class="docs-eyebrow">
          frp.sh / {{ lang === "en" ? "FIELD GUIDE" : "使用文档" }}
        </p></template
      >
    </DefaultTheme.Layout>
  </div>
</template>
