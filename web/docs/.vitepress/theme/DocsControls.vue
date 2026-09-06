<script setup>
import { computed, ref, onMounted, onBeforeUnmount } from "vue";
import { useData, useRoute } from "vitepress";
import { createThemeReveal } from "./theme-reveal.js";
let themeReveal;
const { lang, isDark } = useData();
const route = useRoute();
const mode = ref("auto");
const english = computed(() => lang.value === "en");
const localeLink = computed(() =>
  english.value ? route.path.replace(/^\/en\//, "/") : "/en" + route.path,
);
const label = computed(() =>
  mode.value === "auto"
    ? english.value
      ? "Auto"
      : "跟随系统"
    : mode.value === "day"
      ? english.value
        ? "Day"
        : "白天"
      : english.value
        ? "Night"
        : "夜晚",
);
let media;
function apply() {
  isDark.value =
    mode.value === "night" || (mode.value === "auto" && media.matches);
}
function cycle(event) {
  themeReveal.reveal(event, () => {
    const modes = ["auto", "day", "night"];
    mode.value = modes[(modes.indexOf(mode.value) + 1) % 3];
    try {
      localStorage.setItem("frpsh-world-light", mode.value);
    } catch {}
    apply();
  });
}
function remember() {
  try {
    localStorage.setItem("frpsh-site-language", english.value ? "zh-CN" : "en");
  } catch {}
}
onMounted(() => {
  themeReveal = createThemeReveal();
  try {
    const saved = localStorage.getItem("frpsh-world-light");
    if (["auto", "day", "night"].includes(saved)) mode.value = saved;
  } catch {}
  media = matchMedia("(prefers-color-scheme: dark)");
  media.addEventListener("change", apply);
  apply();
});
onBeforeUnmount(() => {
  themeReveal?.dispose();
  media?.removeEventListener("change", apply);
});
</script>
<template>
  <div class="docs-controls">
    <a
      :href="localeLink"
      @click="remember"
      :aria-label="english ? '切换到中文' : 'Switch to English'"
      >{{ english ? "EN / 中文" : "中文 / EN" }}</a
    ><button
      @click="cycle"
      :aria-label="english ? 'Change appearance' : '切换外观'"
    >
      {{ label }}
    </button>
  </div>
</template>
