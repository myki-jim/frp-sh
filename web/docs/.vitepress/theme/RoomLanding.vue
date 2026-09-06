<script setup>
import SiteIcon from "./SiteIcon.vue";
import {
  ref,
  reactive,
  onMounted,
  onBeforeUnmount,
  watch,
  nextTick,
} from "vue";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { createThemeReveal } from "./theme-reveal.js";
let themeReveal;
import { commands } from "./install-commands.js";
import "./rooms.css";
const root = ref(null),
  host = ref(null),
  language = ref("en"),
  mode = ref("day"),
  platform = ref("unix"),
  message = ref(""),
  failed = ref(false);
const scene = reactive({ night: false, reduced: false });
const motion = { progress: 0 };
const tr = (en, zh) => (language.value === "en" ? en : zh);
const docs = (path) => (language.value === "en" ? "/en/" : "/") + path;
let world,
  media,
  ctx,
  stop,
  timer,
  disposed = false;
function save(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}
function toggleLanguage(event) {
  themeReveal.reveal(
    event,
    async () => {
      language.value = language.value === "en" ? "zh-CN" : "en";
      document.documentElement.lang = language.value;
      save("frpsh-site-language", language.value);
      message.value = "";
      await nextTick();
      ScrollTrigger.refresh();
    },
    "language",
  );
}
function light(event, target) {
  if (mode.value === target) return;
  themeReveal.reveal(event, () => {
    mode.value = target;
    save("frpsh-world-light", mode.value);
    preferences();
  });
}
function preferences() {
  scene.night = mode.value === "night";
  scene.reduced = media.matches;
  world?.refresh();
}
async function copy() {
  try {
    await navigator.clipboard.writeText(commands[platform.value]);
    message.value = tr("Copied.", "已复制。");
  } catch {
    message.value = tr(
      "Select the command to copy it.",
      "请选择指令手动复制。",
    );
  }
  clearTimeout(timer);
  timer = setTimeout(() => (message.value = ""), 3500);
}
onMounted(async () => {
  themeReveal = createThemeReveal();
  try {
    if (localStorage.getItem("frpsh-site-language") === "zh-CN")
      language.value = "zh-CN";
    const m = localStorage.getItem("frpsh-world-light");
    if (["day", "night"].includes(m)) mode.value = m;
  } catch {}
  document.documentElement.lang = language.value;
  media = matchMedia("(prefers-reduced-motion: reduce)");
  preferences();
  media.addEventListener("change", preferences);
  gsap.registerPlugin(ScrollTrigger);
  ctx = gsap.matchMedia();
  ctx.add(
    {
      desktop: "(min-width: 851px)",
      mobile: "(max-width: 850px)",
      reduced: "(prefers-reduced-motion: reduce)",
    },
    ({ conditions }) => {
      if (conditions.reduced) {
        motion.progress = 1;
        return;
      }
      const desktop = conditions.desktop;
      const entrance = gsap.timeline({ defaults: { ease: "power4.out" } });
      entrance
        .from(".headline-line > span", {
          yPercent: 115,
          rotation: 5,
          duration: 1.3,
          stagger: 0.16,
        })
        .from(
          ".hero-copy .eyebrow, .intro, .hero-actions",
          { y: 45, opacity: 0, duration: 0.9, stagger: 0.1 },
          0.3,
        );
      gsap.fromTo(
        ".studio-shell",
        { clipPath: "inset(18% 12% 10% 12%)", y: 100, scale: 0.86 },
        {
          clipPath: "inset(0% 0% 0% 0%)",
          y: 0,
          scale: 1,
          ease: "none",
          scrollTrigger: {
            trigger: ".studio-shell",
            start: "top 95%",
            end: "top 22%",
            scrub: 0.8,
          },
        },
      );
      gsap.fromTo(
        motion,
        { progress: 0 },
        {
          progress: 1,
          ease: "none",
          scrollTrigger: {
            trigger: ".studio-shell",
            start: "top bottom",
            end: "center 30%",
            scrub: 1,
          },
        },
      );
      gsap.from(".connection h2", {
        x: desktop ? -160 : -65,
        opacity: 0,
        duration: 1.15,
        scrollTrigger: {
          trigger: ".connection",
          start: "top 78%",
          toggleActions: "play none none reverse",
        },
      });
      gsap.from(".connection-copy", {
        x: desktop ? 120 : 55,
        opacity: 0,
        duration: 1.15,
        scrollTrigger: {
          trigger: ".connection",
          start: "top 75%",
          toggleActions: "play none none reverse",
        },
      });
      gsap.from(".route i", {
        scaleX: 0,
        transformOrigin: "left",
        duration: 1.2,
        scrollTrigger: {
          trigger: ".route",
          start: "top 85%",
          toggleActions: "play none none reverse",
        },
      });
      gsap.fromTo(
        ".motion-track",
        { xPercent: 12 },
        {
          xPercent: -35,
          ease: "none",
          scrollTrigger: {
            trigger: ".motion-band",
            start: "top bottom",
            end: "bottom top",
            scrub: 1,
          },
        },
      );
      gsap.utils.toArray(".use-grid article").forEach((el, i) => {
        gsap.from(el, {
          x: (i ? 1 : -1) * (desktop ? 170 : 65),
          y: 90,
          rotation: i ? 5 : -5,
          opacity: 0,
          ease: "power3.out",
          duration: 1.2,
          scrollTrigger: {
            trigger: el,
            start: "top 88%",
            toggleActions: "play none none reverse",
          },
        });
      });
      gsap.from(".details > *", {
        y: 85,
        opacity: 0,
        stagger: 0.15,
        duration: 1,
        scrollTrigger: { trigger: ".details", start: "top 85%", once: true },
      });
      gsap.from(".install h2", {
        y: 90,
        opacity: 0,
        duration: 1.1,
        scrollTrigger: { trigger: ".install", start: "top 90%", once: true },
      });
      gsap.from(".install-box", {
        y: 100,
        rotationX: 14,
        scale: 0.9,
        opacity: 0,
        duration: 1.15,
        clearProps: "transform,opacity",
        scrollTrigger: { trigger: ".install", start: "top 90%", once: true },
      });
    },
    root.value,
  );
  try {
    const { mountStudios } = await import("./studios.js");
    if (disposed) return;
    world = mountStudios(host.value, scene, motion);
    stop = watch(
      () => [scene.night, scene.reduced],
      () => world?.refresh(),
    );
  } catch (e) {
    failed.value = true;
    console.error(e);
  }
});
onBeforeUnmount(() => {
  disposed = true;
  themeReveal?.dispose();
  world?.dispose();
  ctx?.revert();
  stop?.();
  clearTimeout(timer);
  media?.removeEventListener("change", preferences);
});
</script>
<template>
  <main
    ref="root"
    class="site"
    :class="{ night: scene.night }"
    :lang="language"
  >
    <header class="site-header">
      <a href="/" class="wordmark">frp.sh</a>
      <nav>
        <a :href="docs('quickstart')">{{ tr("Guide", "指南") }}</a
        ><a href="https://github.com/myki-jim/frp-sh">GitHub <SiteIcon /></a
        ><button
          @click="toggleLanguage"
          :aria-label="tr('切换到中文', 'Switch to English')"
        >
          {{ language === "en" ? "EN / 中文" : "中文 / EN" }}
        </button>
        <div
          class="appearance-options"
          role="group"
          :aria-label="tr('Appearance', '外观')"
        >
          <button
            @click="light($event, 'day')"
            :aria-pressed="mode === 'day'"
            :aria-label="tr('Day', '白天')"
            :title="tr('Day', '白天')"
          >
            <SiteIcon name="sun" />
          </button>
          <button
            @click="light($event, 'night')"
            :aria-pressed="mode === 'night'"
            :aria-label="tr('Night', '黑夜')"
            :title="tr('Night', '黑夜')"
          >
            <SiteIcon name="moon" />
          </button>
        </div>
      </nav>
    </header>
    <section class="hero">
      <div class="hero-copy">
        <p class="eyebrow">P2P NETWORKING / v0.4.0</p>
        <h1>
          <span class="headline-line"
            ><span>{{ tr("Your people.", "你和伙伴，") }}</span></span
          >
          <span class="headline-line headline-muted"
            ><span>{{ tr("One network.", "同一个网络。") }}</span></span
          >
        </h1>
        <p class="intro">
          {{
            tr(
              "A room code brings your devices together. Direct connections for building, playing, and everything in between.",
              "一个房间号，让大家的设备连在一起。直连优先，一起开发，一起开玩。",
            )
          }}
        </p>
        <div class="hero-actions">
          <a href="#install" class="primary"
            >{{ tr("Get frp-sh", "获取 frp-sh") }} <span><SiteIcon /></span></a
          ><a :href="docs('architecture')" class="text-link"
            >{{ tr("How it connects", "了解连接原理") }} <SiteIcon
          /></a>
        </div>
      </div>
      <div class="studio-shell">
        <div class="network-legend" aria-hidden="true">
          <span>● {{ tr("SHARED ROOM", "共享房间") }}</span
          ><span>06 {{ tr("SPACES / ONE NETWORK", "空间 / 同一个网络") }}</span>
        </div>
        <div
          ref="host"
          class="studios"
          role="img"
          :aria-label="
            tr(
              'Six distinct workspaces connected in one network',
              '六个不同风格的工作空间连接在同一个网络中',
            )
          "
        ></div>
        <div v-if="failed" class="studio-fallback">A — B — C<br />ONE ROOM</div>
        <div class="model-caption">
          <span>{{
            tr(
              "Different places. Shared possibilities.",
              "身在各处，一起创造。",
            )
          }}</span
          ><span>{{ tr("Connection illustration", "连接概念示意") }}</span>
        </div>
      </div>
    </section>
    <div class="network-steps">
      <span>01 / {{ tr("DISCOVER", "发现伙伴") }}</span
      ><i><SiteIcon /></i><span>02 / {{ tr("CONNECT", "建立连接") }}</span
      ><i><SiteIcon /></i><span>03 / {{ tr("TOGETHER", "一起开始") }}</span>
    </div>
    <section class="connection reveal">
      <div>
        <p class="eyebrow">01 / {{ tr("THE CONNECTION", "连接") }}</p>
        <h2>
          {{ tr("Less waiting.", "少一点等待。") }}<br />{{
            tr("More doing.", "多一点开始。")
          }}
        </h2>
      </div>
      <div class="connection-copy">
        <p>
          {{
            tr(
              "Parallel address discovery looks for a direct path between devices. When a network gets in the way, TURN or TCP relay provides another route.",
              "并行地址探测寻找设备间的直接路径。遇到网络限制时，通过 TURN 或 TCP 中继接力。",
            )
          }}
        </p>
        <div class="route">
          <span>YOU</span><i></i><span>{{ tr("YOUR ROOM", "你的房间") }}</span>
        </div>
        <a :href="docs('architecture')" class="text-link"
          >{{ tr("Read the architecture", "阅读网络原理") }} <SiteIcon
        /></a>
      </div>
    </section>
    <div class="motion-band" aria-hidden="true">
      <div class="motion-track">
        <span>{{ tr("LESS WAITING", "少一点等待") }}</span
        ><i><SiteIcon /></i><span>{{ tr("MORE TOGETHER", "一起，即刻") }}</span
        ><i><SiteIcon /></i>
      </div>
    </div>
    <section class="use-section">
      <div class="section-line reveal">
        <p class="eyebrow">02 / {{ tr("MADE FOR TOGETHER", "为一起而生") }}</p>
        <span>{{
          tr("One room. More than two people.", "一个房间，不止两个人。")
        }}</span>
      </div>
      <div class="use-grid">
        <article class="reveal">
          <span class="use-number">A.</span>
          <h2>{{ tr("Build together.", "一起创造。") }}</h2>
          <p>
            {{
              tr(
                "Let teammates reach your local service. Share what you’re working on without moving it somewhere else.",
                "让伙伴访问你的本地服务。项目留在本机，想法直接分享。",
              )
            }}
          </p>
          <code>frp-sh dev create --service 127.0.0.1:3000</code>
        </article>
        <article class="reveal">
          <span class="use-number">B.</span>
          <h2>{{ tr("Play together.", "一起开玩。") }}</h2>
          <p>
            {{
              tr(
                "Bring friends into a virtual LAN. Share the room code and meet on the same server.",
                "邀请朋友加入虚拟局域网。分享房间号，在同一个服务器相遇。",
              )
            }}
          </p>
          <code>frp-sh game create --service 127.0.0.1:25565</code>
        </article>
      </div>
    </section>
    <section class="details reveal">
      <p>{{ tr("Small by design.", "轻量，是设计的一部分。") }}</p>
      <div>
        <h3>{{ tr("Authorize at installation.", "安装时授权。") }}</h3>
        <p>
          {{
            tr(
              "The helper handles privileged networking. Everyday connections run from your ordinary account. Updates use the installer.",
              "辅助服务处理特权网络操作，日常连接使用普通账户，升级通过安装器完成。",
            )
          }}
        </p>
      </div>
      <div>
        <h3>{{ tr("Clear when it matters.", "需要时，看得清。") }}</h3>
        <p>
          {{
            tr(
              "An uncluttered terminal, separate logs, and open-source Rust you can inspect.",
              "简洁终端、独立日志，以及随时可以审视的 Rust 开源代码。",
            )
          }}
        </p>
      </div>
    </section>
    <section id="install" class="install reveal">
      <div>
        <p class="eyebrow">03 / {{ tr("GET STARTED", "开始") }}</p>
        <h2>
          {{ tr("See you", "房间里，") }}<br />{{ tr("in the room.", "见。") }}
        </h2>
        <a :href="docs('install')" class="text-link"
          >{{ tr("Installation guide", "安装指南") }} <SiteIcon
        /></a>
      </div>
      <div class="install-box">
        <div
          class="platforms"
          role="group"
          :aria-label="tr('Operating system', '操作系统')"
        >
          <button
            :aria-pressed="platform === 'unix'"
            @click="
              platform = 'unix';
              message = '';
            "
          >
            macOS / Linux</button
          ><button
            :aria-pressed="platform === 'windows'"
            @click="
              platform = 'windows';
              message = '';
            "
          >
            Windows</button
          ><span>v0.4.0</span>
        </div>
        <pre>{{ commands[platform] }}</pre>
        <div class="copy-row">
          <span role="status">{{ message }}</span
          ><button @click="copy">
            {{ tr("Copy command", "复制指令") }} <SiteIcon />
          </button>
        </div>
        <p class="install-note">
          {{
            tr(
              "Installs a privileged helper. Then run frp-sh to configure your signaling server.",
              "安装会配置特权辅助服务。随后运行 frp-sh，配置你的信令服务器。",
            )
          }}
        </p>
      </div>
    </section>
    <footer>
      <a href="/" class="wordmark">frp.sh</a
      ><a href="https://github.com/myki-jim/frp-sh/releases/tag/v0.4.0"
        >{{ tr("Downloads", "下载") }} <SiteIcon /></a
      ><a :href="docs('versioning')">{{ tr("Version policy", "版本策略") }}</a
      ><span>OPEN SOURCE / MIT</span>
    </footer>
  </main>
</template>
