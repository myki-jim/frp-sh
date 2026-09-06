<script setup>
import { ref, computed, onMounted, onBeforeUnmount, nextTick } from "vue";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import RoomScene from "./RoomScene.vue";
import "./rooms.css";
const root = ref(null),
  language = ref("en"),
  progress = ref(0),
  reduced = ref(false),
  platform = ref("unix"),
  copied = ref(false),
  copyFailed = ref(false);
const tr = (en, zh) => (language.value === "en" ? en : zh);
const docs = (path) => (language.value === "en" ? "/en/" : "/") + path;
const phase = computed(() =>
  progress.value < 0.2 ? 0 : progress.value < 0.7 ? 1 : 2,
);
const steps = computed(() => [
  tr("Two places.", "两个地方。"),
  tr("One room code.", "一个房间号。"),
  tr("Connected.", "现在，连在一起。"),
]);
const command = computed(() =>
  platform.value === "unix"
    ? "curl -fsSL https://frp.sh/install.sh | sh"
    : "irm https://frp.sh/install.ps1 | iex",
);
let mm, timer, media, replay;
async function toggle() {
  const y = scrollY;
  language.value = language.value === "en" ? "zh-CN" : "en";
  try {
    localStorage.setItem("frpsh-site-language", language.value);
  } catch {}
  document.documentElement.lang = language.value;
  await nextTick();
  ScrollTrigger.refresh();
  window.scrollTo({ top: y, behavior: "instant" });
}
async function copy() {
  try {
    await navigator.clipboard.writeText(command.value);
    copied.value = true;
    copyFailed.value = false;
    clearTimeout(timer);
    timer = setTimeout(() => (copied.value = false), 2000);
  } catch {
    copyFailed.value = true;
  }
}
function demonstrate() {
  replay?.kill();
  progress.value = 0;
  replay = gsap.to(progress, {
    value: 1,
    duration: reduced.value ? 0 : 3.5,
    ease: "power1.inOut",
  });
}
function updatePreference() {
  reduced.value = media.matches;
}
onMounted(() => {
  try {
    if (localStorage.getItem("frpsh-site-language") === "zh-CN")
      language.value = "zh-CN";
  } catch {}
  document.documentElement.lang = language.value;
  gsap.registerPlugin(ScrollTrigger);
  media = matchMedia("(prefers-reduced-motion: reduce)");
  updatePreference();
  media.addEventListener("change", updatePreference);
  mm = gsap.matchMedia();
  mm.add(
    "(prefers-reduced-motion: no-preference)",
    () => {
      gsap.from(".room-title span", {
        yPercent: 110,
        stagger: 0.1,
        duration: 1,
        ease: "power3.out",
      });
      gsap.from(".room-caption", {
        opacity: 0,
        y: 16,
        delay: 0.25,
        duration: 0.8,
      });
      gsap.utils
        .toArray(".editorial-reveal")
        .forEach((el) =>
          gsap.from(el, {
            y: 35,
            opacity: 0,
            duration: 0.8,
            scrollTrigger: { trigger: el, start: "top 88%", once: true },
          }),
        );
    },
    root.value,
  );
  mm.add(
    "(min-width: 800px) and (min-height: 650px) and (prefers-reduced-motion: no-preference)",
    () => {
      ScrollTrigger.create({
        trigger: ".room-stage",
        start: "top top",
        end: "+=1500",
        pin: true,
        scrub: true,
        onUpdate: (self) => {
          replay?.kill();
          progress.value = self.progress;
        },
      });
      const strip = root.value.querySelector(".usecase-strip");
      gsap.to(strip, {
        x: () =>
          -Math.max(
            0,
            strip.scrollWidth -
              root.value.querySelector(".usecases").clientWidth,
          ),
        ease: "none",
        scrollTrigger: {
          trigger: ".usecases",
          start: "top top",
          end: "+=900",
          pin: true,
          scrub: 1,
          invalidateOnRefresh: true,
        },
      });
    },
    root.value,
  );
});
onBeforeUnmount(() => {
  mm?.revert();
  replay?.kill();
  clearTimeout(timer);
  media?.removeEventListener("change", updatePreference);
});
</script>
<template>
  <div ref="root" class="rooms-page" :lang="language">
    <a href="#room-content" class="room-skip">{{
      tr("Skip to content", "跳到正文")
    }}</a>
    <header class="room-header">
      <a href="/" class="room-brand"
        >frp.sh<span class="brand-squares" aria-hidden="true"
          ><i></i><i></i></span
      ></a>
      <nav :aria-label="tr('Main navigation', '主导航')">
        <a :href="docs('quickstart')">{{ tr("Field guide", "使用指南") }}</a
        ><a href="https://github.com/myki-jim/frp-sh">GitHub ↗</a
        ><button
          @click="toggle"
          :aria-label="tr('切换到中文', 'Switch to English')"
        >
          <span :class="{ selected: language === 'en' }">EN</span
          ><span class="lang-separator">/</span
          ><span :class="{ selected: language === 'zh-CN' }">中文</span>
        </button>
      </nav>
      <a class="header-install" href="#install"
        >{{ tr("Get frp-sh", "获取 frp-sh") }} <span>↗</span></a
      >
    </header>
    <main id="room-content">
      <section
        class="room-stage"
        :aria-label="tr('An illustrated connection', '连接场景演示')"
      >
        <div class="room-title-row">
          <h1 class="room-title">
            <span>{{
              tr("Your place. Their place.", "你的房间，他的房间。")
            }}</span
            ><span class="title-second">{{
              tr("Same room.", "现在，同一个房间。")
            }}</span>
          </h1>
          <div class="room-caption">
            <span class="small-label">{{
              tr("A LITTLE LESS FAR AWAY", "让远方，近一点")
            }}</span>
            <p>
              {{
                tr(
                  "A peer-to-peer tunnel for the things you do together. Create a room. Share its code.",
                  "为一起做的事，建立 P2P 隧道。创建房间，分享号码，就这么开始。",
                )
              }}
            </p>
            <a href="#install"
              >{{ tr("Make a connection", "建立你的连接") }} <span>↗</span></a
            >
          </div>
        </div>
        <div class="model-wrap">
          <RoomScene
            :progress="progress"
            :reduced="reduced"
            :language="language"
          />
          <div class="model-coordinate coordinate-a">
            <b>A</b><span>{{ tr("YOUR DESK", "你的桌面") }}</span>
          </div>
          <div class="model-coordinate coordinate-b">
            <b>B</b><span>{{ tr("SOMEWHERE ELSE", "另一个地方") }}</span>
          </div>
          <span class="model-note">{{
            tr(
              "AN INTERACTIVE MODEL, NOT LIVE NETWORK STATUS",
              "交互模型演示，并非实时网络状态",
            )
          }}</span>
        </div>
        <div class="room-stage-footer">
          <div class="stage-chapter">
            <span class="chapter-number">0{{ phase + 1 }}</span>
            <div>
              <span class="small-label">{{
                tr("THE CONNECTION", "连接发生的过程")
              }}</span>
              <h2>{{ steps[phase] }}</h2>
            </div>
          </div>
          <div class="room-code">
            <span>{{ tr("ROOM", "房间") }}</span
            ><strong>{{ phase === 0 ? "— — — —" : "orbit-2048" }}</strong
            ><i :class="{ connected: phase === 2 }"></i>
          </div>
          <button class="replay" @click="demonstrate">
            {{ tr("Play the connection", "播放连接过程") }}
            <span>↗</span></button
          ><span class="scroll-hint"
            >{{
              tr("SCROLL TO BRING THEM TOGETHER", "滚动，让房间靠近")
            }}
            ↓</span
          >
        </div>
      </section>
      <section class="room-essay editorial-reveal">
        <span class="small-label">{{
          tr("NETWORKING, WITH THE DISTANCE TAKEN OUT.", "网络，还可以更简单。")
        }}</span>
        <h2>
          {{ tr("You have better things", "你有更重要的事，") }}<br />{{
            tr("to do than configure a network.", "不必困在网络配置里。")
          }}
        </h2>
        <div class="essay-bottom">
          <span class="orange-asterisk" aria-hidden="true">✳</span>
          <p>
            {{
              tr(
                "A game night. A side project. A quick look at what you’re building. frp-sh connects your machines so you can get back to whatever brought you together.",
                "一场游戏，一个业余项目，或是一起看看刚写好的功能。frp-sh 连接你的设备，让你回到最初想一起做的事。",
              )
            }}
          </p>
        </div>
      </section>
      <section class="connection-details">
        <div class="editorial-reveal">
          <span class="small-label">{{
            tr("HOW IT FINDS A WAY", "连接如何找到路")
          }}</span>
          <h2>
            {{ tr("Straight there.", "直达对端。") }}<br /><em>{{
              tr("Or around the corner.", "也能绕过阻碍。")
            }}</em>
          </h2>
          <p>
            {{
              tr(
                "Parallel address discovery looks for a direct UDP path. When the network won’t allow it, TURN or TCP relay provides another way through.",
                "并行地址探测寻找 UDP 直连路径。当网络条件受限时，自动通过 TURN 或 TCP 中继寻找另一条路。",
              )
            }}
          </p>
          <a :href="docs('architecture')">{{
            tr("Inside the protocol ↗", "了解连接原理 ↗")
          }}</a>
        </div>
        <div
          class="route-drawing"
          role="img"
          :aria-label="
            tr(
              'A direct route and an alternative relay route',
              '直连路径和备用中继路径',
            )
          "
        >
          <span class="route-end">A</span
          ><svg viewBox="0 0 460 280" aria-hidden="true">
            <path class="route-alternate" d="M30 140V50H430V140" />
            <path class="route-direct" d="M30 140H430" />
            <circle cx="230" cy="50" r="9" />
            <rect x="216" y="126" width="28" height="28" rx="4" /></svg
          ><span class="route-end">B</span
          ><span class="route-label relay-label">{{
            tr("RELAY, IF NEEDED", "必要时中继")
          }}</span
          ><span class="route-label direct-label">{{
            tr("DIRECT, WHEN POSSIBLE", "可直连，就直连")
          }}</span>
        </div>
      </section>
      <section class="usecases">
        <div class="usecase-strip">
          <article class="usecase-title">
            <span class="small-label">{{
              tr("THINGS WORTH CONNECTING FOR", "值得连接的那些事")
            }}</span>
            <h2>
              {{ tr("A small tool.", "一个小工具。") }}<br />{{
                tr("A wider world.", "更大的世界。")
              }}
            </h2>
            <span class="large-arrow">⟶</span>
          </article>
          <article class="usecase-card">
            <span class="small-label">{{
              tr("01 / MAKE SOMETHING", "01 / 一起创造")
            }}</span>
            <div class="mini-window">
              <div><i></i><span>localhost:3000</span></div>
              <pre><span>❯</span> frp-sh dev create
  --service 127.0.0.1:3000

<b>{{tr('Room ready.','房间已就绪。')}}</b>
  {{tr('Send the code to a teammate.','把房间号发给伙伴。')}}</pre>
            </div>
            <h3>{{ tr("“Can you take a look?”", "“帮我看一眼？”") }}</h3>
            <p>
              {{
                tr(
                  "Give a teammate a way to reach your local service. Keep the work where it is.",
                  "让伙伴访问你的本地服务。作品无需搬家，想法即刻分享。",
                )
              }}
            </p>
          </article>
          <article class="usecase-card game-card">
            <span class="small-label">{{
              tr("02 / MAKE AN EVENING OF IT", "02 / 一起开玩")
            }}</span>
            <div class="game-board" aria-hidden="true">
              <div class="game-piece piece-a">A</div>
              <div class="game-route"></div>
              <div class="game-piece piece-b">B</div>
              <span>PLAYER 01 + PLAYER 02</span>
            </div>
            <h3>
              {{
                tr("Same server. Same evening.", "同一个服务器，同一个游戏夜。")
              }}
            </h3>
            <p>
              {{
                tr(
                  "Bring friends into a virtual LAN. The room code is your invitation.",
                  "把朋友带进虚拟局域网，房间号就是邀请函。",
                )
              }}
            </p>
          </article>
        </div>
      </section>
      <section class="room-facts">
        <h2 class="editorial-reveal">
          {{ tr("Less in the way.", "少一点负担。") }}
        </h2>
        <div class="facts-grid">
          <article class="editorial-reveal">
            <strong>38.7<small>%</small></strong>
            <h3>{{ tr("Smaller, together.", "加在一起，依然更小。") }}</h3>
            <p>
              {{
                tr(
                  "Windows client and helper versus the previous main binary. Release builds; excludes Wintun, configuration and backups.",
                  "Windows 客户端与辅助服务合计，相比旧主程序的 Release 构建体积。不含 Wintun、配置和备份。",
                )
              }}
            </p>
          </article>
          <article class="editorial-reveal">
            <strong>01</strong>
            <h3>{{ tr("Authorize at installation.", "安装时授权。") }}</h3>
            <p>
              {{
                tr(
                  "The network helper handles privileged operations. Everyday connections run from your ordinary account. Updates use the installer.",
                  "网络辅助服务处理特权操作，日常连接使用普通账户。升级仍通过安装器完成。",
                )
              }}
            </p>
          </article>
          <article class="editorial-reveal">
            <strong>{ }</strong>
            <h3>{{ tr("Yours to inspect.", "代码，交给你审视。") }}</h3>
            <p>
              {{
                tr(
                  "Open-source Rust. Optional shared-key end-to-end encryption. Separate logs when you need to look closer.",
                  "Rust 开源实现，可选共享密钥端到端加密。需要深入排查时，独立日志随时可查。",
                )
              }}
            </p>
          </article>
        </div>
      </section>
      <section id="install" class="room-install">
        <div>
          <span class="small-label">{{
            tr("YOUR NEXT ROOM", "你的下一个房间")
          }}</span>
          <h2>{{ tr("Come on in.", "进来坐坐。") }}</h2>
          <p>
            {{
              tr(
                "Install frp-sh. Then run it to configure your signaling server.",
                "安装 frp-sh，然后运行它来配置你的信令服务器。",
              )
            }}
          </p>
          <a :href="docs('install')">{{
            tr("Read the installation guide ↗", "阅读安装指南 ↗")
          }}</a>
        </div>
        <div class="paper-command">
          <div class="os-tabs">
            <button
              :aria-pressed="platform === 'unix'"
              @click="
                platform = 'unix';
                copied = false;
              "
            >
              macOS / Linux</button
            ><button
              :aria-pressed="platform === 'windows'"
              @click="
                platform = 'windows';
                copied = false;
              "
            >
              Windows</button
            ><span>v0.4.0</span>
          </div>
          <div class="install-command">
            <code>{{ command }}</code
            ><button
              @click="copy"
              :aria-label="tr('Copy installation command', '复制安装命令')"
            >
              {{
                copied ? tr("Copied ✓", "已复制 ✓") : tr("Copy ↗", "复制 ↗")
              }}
            </button>
          </div>
          <p role="status">
            {{
              copyFailed
                ? tr(
                    "Select and copy the command above.",
                    "请选择并复制上方命令。",
                  )
                : copied
                  ? tr("Command copied.", "命令已复制。")
                  : tr(
                      "Installs a privileged helper. Review the script before running.",
                      "安装会配置特权辅助服务，请先检查脚本。",
                    )
            }}
          </p>
        </div>
      </section>
    </main>
    <footer class="room-footer">
      <a class="room-brand" href="/">frp.sh</a
      ><span>{{ tr("A LITTLE LESS FAR AWAY.", "让远方，近一点。") }}</span
      ><a href="https://github.com/myki-jim/frp-sh/releases/tag/v0.4.0">{{
        tr("Downloads ↗", "全部下载 ↗")
      }}</a
      ><a :href="docs('versioning')">{{
        tr("0.x version policy", "0.x 版本策略")
      }}</a
      ><span>MIT</span>
    </footer>
  </div>
</template>
