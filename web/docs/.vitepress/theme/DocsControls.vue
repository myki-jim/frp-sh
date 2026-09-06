<script setup>
import SiteIcon from "./SiteIcon.vue";
import { computed, ref, onMounted, onBeforeUnmount } from "vue";
import { useData, useRoute, useRouter } from "vitepress";
import { createThemeReveal } from "./theme-reveal.js";
let themeReveal;
const { lang, isDark } = useData();
const route = useRoute();
const router = useRouter();
const mode = ref("day");
const english = computed(() => lang.value === "en");
const localeLink = computed(() =>
  english.value ? route.path.replace(/^\/en\//, "/") : "/en" + route.path,
);
function select(event, target) {
  if (mode.value === target) return;
  themeReveal.reveal(event, () => {
    mode.value = target;
    try {
      localStorage.setItem("frpsh-world-light", target);
    } catch {}
    isDark.value = target === "night";
  });
}
function language(event) {
  if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  const target = localeLink.value;
  const preference = english.value ? "zh-CN" : "en";
  themeReveal.reveal(
    event,
    async () => {
      try {
        localStorage.setItem("frpsh-site-language", preference);
      } catch {}
      await router.go(target);
    },
    "language",
  );
}
onMounted(() => {
  themeReveal = createThemeReveal();
  try {
    const saved = localStorage.getItem("frpsh-world-light");
    if (["day", "night"].includes(saved)) mode.value = saved;
  } catch {}
  isDark.value = mode.value === "night";
});
onBeforeUnmount(() => themeReveal?.dispose());
</script>
<template>
  <div class="docs-controls">
    <a
      :href="localeLink"
      @click="language"
      :aria-label="english ? '切换到中文' : 'Switch to English'"
      >{{ english ? "EN / 中文" : "中文 / EN" }}</a
    >
    <div
      class="appearance-options"
      role="group"
      :aria-label="english ? 'Appearance' : '外观'"
    >
      <button
        @click="select($event, 'day')"
        :aria-pressed="mode === 'day'"
        :aria-label="english ? 'Day' : '白天'"
        :title="english ? 'Day' : '白天'"
      >
        <SiteIcon name="sun" />
      </button>
      <button
        @click="select($event, 'night')"
        :aria-pressed="mode === 'night'"
        :aria-label="english ? 'Night' : '黑夜'"
        :title="english ? 'Night' : '黑夜'"
      >
        <SiteIcon name="moon" />
      </button>
    </div>
  </div>
</template>
