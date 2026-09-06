<script setup>
import { useData } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { defineAsyncComponent, watchEffect, onMounted } from "vue";
import DocsControls from "./DocsControls.vue";
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
  <Landing v-if="frontmatter.cinematic" />
  <div v-else class="doc-shell">
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
