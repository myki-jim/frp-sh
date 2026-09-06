<script setup>
import { ref, reactive, onMounted, onBeforeUnmount, watch } from "vue";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { chapters, chapterAt, stops, commands } from "./room-story.js";
import "./rooms.css";
const root = ref(null),
  host = ref(null),
  ready = ref(false),
  failed = ref(false),
  chapter = ref(0);
const state = reactive({
  progress: 0,
  language: "en",
  mode: "auto",
  systemDark: false,
  reduced: false,
  platform: "unix",
  message: "",
});
const tr = (en, zh) => (state.language === "zh-CN" ? zh : en);
let world,
  trigger,
  media,
  dark,
  context,
  timer,
  disposed = false,
  stopWatch;
function remember(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}
function go(p) {
  if (!root.value) return;
  const total = root.value.offsetHeight - innerHeight;
  window.scrollTo({
    top: root.value.offsetTop + total * p,
    behavior: state.reduced ? "instant" : "smooth",
  });
}
async function act(id) {
  if (id === "language") {
    state.language = state.language === "en" ? "zh-CN" : "en";
    state.message = "";
    remember("frpsh-site-language", state.language);
    document.documentElement.lang = state.language;
  } else if (id === "day") {
    const modes = ["auto", "day", "night"];
    state.mode = modes[(modes.indexOf(state.mode) + 1) % 3];
    remember("frpsh-world-light", state.mode);
  } else if (id === "install") go(1);
  else if (id === "previous")
    go(stops[Math.max(0, chapterAt(state.progress) - 1)]);
  else if (id === "next") go(stops[Math.min(7, chapterAt(state.progress) + 1)]);
  else if (id === "platform") {
    state.platform = state.platform === "unix" ? "windows" : "unix";
    state.message = "";
  } else if (id === "copy") {
    try {
      await navigator.clipboard.writeText(commands[state.platform]);
      state.message = tr("Copied. See you in the room.", "已复制。房间里见。");
    } catch {
      state.message = tr(
        "Select the command above and copy it manually.",
        "请选择上方指令手动复制。",
      );
    }
    clearTimeout(timer);
    timer = setTimeout(() => (state.message = ""), 5000);
  } else if (id === "guide")
    window.location.assign(
      (state.language === "en" ? "/en/" : "/") + "install",
    );
  else if (id === "source")
    window.location.assign("https://github.com/myki-jim/frp-sh");
  else if (id === "webgl-lost") {
    failed.value = true;
    world?.dispose();
    world = null;
  }
  world?.refresh();
}
function preferences() {
  state.reduced = media.matches;
  state.systemDark = dark.matches;
  if (state.reduced) {
    state.progress = stops.reduce((best, value) =>
      Math.abs(value - state.progress) < Math.abs(best - state.progress) ? value : best, 0);
  }
  world?.refresh();
}
function keydown(e) {
  if (
    e.altKey ||
    e.ctrlKey ||
    e.metaKey ||
    e.target.closest("button,a,pre,input")
  )
    return;
  if (e.key === "ArrowRight") {
    e.preventDefault();
    act("next");
  }
  if (e.key === "ArrowLeft") {
    e.preventDefault();
    act("previous");
  }
  if (e.key === "End") {
    e.preventDefault();
    go(1);
  }
  if (e.key === "Home") {
    e.preventDefault();
    go(0);
  }
}
onMounted(async () => {
  try {
    if (localStorage.getItem("frpsh-site-language") === "zh-CN")
      state.language = "zh-CN";
    const saved = localStorage.getItem("frpsh-world-light");
    if (["auto", "day", "night"].includes(saved)) state.mode = saved;
  } catch {}
  document.documentElement.lang = state.language;
  media = matchMedia("(prefers-reduced-motion: reduce)");
  dark = matchMedia("(prefers-color-scheme: dark)");
  preferences();
  media.addEventListener("change", preferences);
  dark.addEventListener("change", preferences);
  window.addEventListener("keydown", keydown);
  gsap.registerPlugin(ScrollTrigger);
  context = gsap.context(() => {
    trigger = ScrollTrigger.create({
      trigger: root.value,
      start: "top top",
      end: "bottom bottom",
      onUpdate: (self) => {
        state.progress = state.reduced
          ? stops.reduce(
              (best, value) =>
                Math.abs(value - self.progress) < Math.abs(best - self.progress)
                  ? value
                  : best,
              0,
            )
          : self.progress;
        if (state.reduced) world?.refresh();
      },
    });
  }, root.value);
  try {
    const { createRoomWorld } = await import("./room-world.js");
    if (disposed) return;
    world = createRoomWorld(
      host.value,
      state,
      act,
      (value) => (chapter.value = value),
    );
    ready.value = true;
    stopWatch = watch(
      () => [
        state.language,
        state.mode,
        state.platform,
        state.message,
        state.reduced,
        state.systemDark,
      ],
      () => world?.refresh(),
    );
  } catch (error) {
    console.error("Room scene could not start", error);
    failed.value = true;
  }
});
onBeforeUnmount(() => {
  disposed = true;
  stopWatch?.();
  world?.dispose();
  context?.revert();
  clearTimeout(timer);
  media?.removeEventListener("change", preferences);
  dark?.removeEventListener("change", preferences);
  window.removeEventListener("keydown", keydown);
});
</script>
<template>
  <main
    ref="root"
    class="room-journey"
    :class="{ 'world-failed': failed }"
    :lang="state.language"
    :data-chapter="chapter"
    :data-light="state.mode"
  >
    <noscript><a href="/en/install">Install frp-sh / 安装指南</a></noscript>
    <div class="world-viewport">
      <div ref="host" class="world-host" :aria-busy="!ready && !failed"></div>
      <div v-if="!ready && !failed" class="world-loading" role="status">
        {{ tr("Opening the room…", "正在打开房间…") }}
      </div>
      <div class="world-access">
        <h1>frp.sh — {{ tr("A room for everyone.", "大家的房间。") }}</h1>
        <p aria-live="polite">
          {{ chapters[chapter][state.language === "en" ? 0 : 1] }}
        </p>
        <p>
          {{
            tr(
              "Scroll to explore. Left and right arrow keys change scenes. End goes to installation. This is an illustrated connection, not live network status.",
              "滚动探索，左右方向键切换场景，End 跳到安装。这是连接概念演示，并非实时网络状态。",
            )
          }}
        </p>
        <button @click="act('install')">
          {{ tr("Skip to installation", "跳到安装") }}
        </button>
        <button @click="act('language')">
          {{ tr("切换到中文", "Switch to English") }}
        </button>
        <button @click="act('day')">
          {{ tr("Change day or night", "切换昼夜模式") }}
        </button>
        <button @click="act('previous')">
          {{ tr("Previous scene", "上一幕") }}
        </button>
        <button @click="act('next')">{{ tr("Next scene", "下一幕") }}</button>
      </div>
      <section v-if="failed" class="world-fallback">
        <h1>frp.sh</h1>
        <p>{{ tr("A room for everyone.", "大家的房间。") }}</p>
        <p>
          {{
            tr(
              "The 3D scene is unavailable on this device. You can still install frp-sh below.",
              "此设备无法显示 3D 场景，你仍可在下方安装 frp-sh。",
            )
          }}
        </p>
        <button @click="act('language')">EN / 中文</button>
        <button @click="act('platform')">
          {{ state.platform === "unix" ? "macOS / Linux" : "Windows" }} ↔
        </button>
        <pre>{{ commands[state.platform] }}</pre>
        <button @click="act('copy')">
          {{ tr("Copy command", "复制命令") }}
        </button>
        <p role="status">{{ state.message }}</p>
        <a :href="(state.language === 'en' ? '/en/' : '/') + 'install'">{{
          tr("Installation guide", "安装指南")
        }}</a>
      </section>
    </div>
  </main>
</template>
