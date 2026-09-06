<script setup>
import { ref, onMounted, onBeforeUnmount, nextTick } from "vue";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import "./landing.css";
import ConnectionField from "./ConnectionField.vue";
import zh from "./home-zh.json";
const language = ref("en");
const t = (text) => (language.value === "zh-CN" ? zh[text] || text : text);
const docLink = (path) => (language.value === "zh-CN" ? "/" : "/en/") + path;
async function toggleLanguage() {
  const scrollPosition = window.scrollY;
  language.value = language.value === "en" ? "zh-CN" : "en";
  try {
    localStorage.setItem("frpsh-site-language", language.value);
  } catch {}
  document.documentElement.lang = language.value;
  await nextTick();
  ScrollTrigger.refresh();
  window.scrollTo({ top: scrollPosition, behavior: "instant" });
}

const root = ref(null);
const platform = ref("unix");
const copied = ref(false);
const copyError = ref(false);
const command = () =>
  platform.value === "windows"
    ? "irm https://frp.sh/install.ps1 | iex"
    : "curl -fsSL https://frp.sh/install.sh | sh";
let mm, copyTimer;
async function copyCommand() {
  try {
    await navigator.clipboard.writeText(command());
    copied.value = true;
    copyError.value = false;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copied.value = false), 2200);
  } catch {
    copyError.value = true;
  }
}
onMounted(() => {
  try {
    if (localStorage.getItem("frpsh-site-language") === "zh-CN")
      language.value = "zh-CN";
  } catch {}
  document.documentElement.lang = language.value;
  gsap.registerPlugin(ScrollTrigger);
  mm = gsap.matchMedia();
  mm.add(
    "(prefers-reduced-motion: no-preference)",
    () => {
      gsap.fromTo(
        ".reading-progress",
        { scaleX: 0 },
        {
          scaleX: 1,
          ease: "none",
          scrollTrigger: { start: 0, end: "max", scrub: true },
        },
      );
      gsap.to(".horizon-ring", {
        rotation: 180,
        scale: 1.18,
        stagger: 0.2,
        ease: "none",
        scrollTrigger: {
          trigger: ".hero",
          start: "top top",
          end: "bottom top",
          scrub: 1,
        },
      });
      gsap.from(".story-copy", {
        y: 35,
        opacity: 0.2,
        stagger: 0.1,
        scrollTrigger: {
          trigger: ".story",
          start: "top 85%",
          end: "top 15%",
          scrub: 1,
        },
      });
      gsap.utils
        .toArray(".scene")
        .forEach((el) =>
          gsap.fromTo(
            el,
            { clipPath: "inset(8% 5% round 50px)" },
            {
              clipPath: "inset(0% 0% round 18px)",
              scrollTrigger: {
                trigger: el,
                start: "top 90%",
                end: "top 30%",
                scrub: 1,
              },
            },
          ),
        );
      gsap.to(".orbit-a", {
        rotation: 325,
        duration: 18,
        repeat: -1,
        ease: "none",
        scrollTrigger: {
          trigger: ".scene-play",
          toggleActions: "play pause resume pause",
        },
      });
      gsap.to(".orbit-b", {
        rotation: -325,
        duration: 23,
        repeat: -1,
        ease: "none",
        scrollTrigger: {
          trigger: ".scene-play",
          toggleActions: "play pause resume pause",
        },
      });
      gsap.from(".spec", {
        clipPath: "inset(0% 0% 100% 0%)",
        stagger: 0.13,
        duration: 1.1,
        scrollTrigger: { trigger: ".spec-grid", start: "top 85%", once: true },
      });
      gsap.from(".hero-line > span", {
        yPercent: 110,
        rotate: 3,
        duration: 1.15,
        stagger: 0.12,
        ease: "power4.out",
      });
      gsap.from(".hero-sub, .hero-actions", {
        opacity: 0,
        y: 20,
        duration: 0.8,
        stagger: 0.12,
        delay: 0.5,
      });
      gsap.to(".hero-art", {
        yPercent: 15,
        scale: 1.12,
        ease: "none",
        scrollTrigger: {
          trigger: ".hero",
          start: "top top",
          end: "bottom top",
          scrub: true,
        },
      });
      gsap.utils.toArray(".reveal").forEach((el) =>
        gsap.from(el, {
          y: 45,
          opacity: 0,
          duration: 0.9,
          scrollTrigger: { trigger: el, start: "top 90%", once: true },
        }),
      );
      gsap.fromTo(
        ".manifesto-fill",
        { backgroundPosition: "100% 0%" },
        {
          backgroundPosition: "0% 0%",
          ease: "none",
          scrollTrigger: {
            trigger: ".manifesto",
            start: "top 70%",
            end: "bottom 65%",
            scrub: 1,
          },
        },
      );
      gsap.fromTo(
        ".portal-art",
        { clipPath: "inset(16% 24% round 120px)", scale: 0.9 },
        {
          clipPath: "inset(0% 0% round 24px)",
          scale: 1,
          ease: "none",
          scrollTrigger: {
            trigger: ".portal",
            start: "top 85%",
            end: "center center",
            scrub: 1,
          },
        },
      );
      gsap.from(".connection-path", {
        strokeDashoffset: 900,
        ease: "none",
        scrollTrigger: {
          trigger: ".portal",
          start: "top 50%",
          end: "bottom 70%",
          scrub: 1,
        },
      });
      gsap.to(".signal-dot", {
        attr: { cx: 870 },
        ease: "none",
        repeat: -1,
        duration: 2.4,
        scrollTrigger: {
          trigger: ".portal-art",
          toggleActions: "play pause resume pause",
        },
      });
    },
    root.value,
  );
  mm.add(
    "(min-width: 900px) and (prefers-reduced-motion: no-preference)",
    () => {
      const track = root.value.querySelector(".story-track");
      gsap.set(track, { display: "flex", width: "max-content" });
      gsap.to(track, {
        x: () => -(track.scrollWidth - window.innerWidth),
        ease: "none",
        scrollTrigger: {
          trigger: ".story",
          start: "top top",
          end: () => "+=" + (track.scrollWidth - window.innerWidth),
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
  clearTimeout(copyTimer);
});
</script>

<template>
  <div ref="root" class="landing" :lang="language">
    <div class="reading-progress" aria-hidden="true"></div>
    <a class="skip-link" href="#experience">{{ t("Skip to content") }}</a>
    <header class="site-header">
      <a class="wordmark" href="/" :aria-label="t('frp-sh home')"
        ><span class="brand-glyph" aria-hidden="true">⌁</span> frp<span
          class="brand-dot"
          >.</span
        >sh</a
      >
      <nav :aria-label="t('Main navigation')">
        <a href="#experience">{{ t("Experience") }}</a
        ><a :href="docLink('quickstart')">{{ t("Documentation ↗") }}</a
        ><button
          class="language-toggle"
          @click="toggleLanguage"
          :aria-label="language === 'en' ? '切换到中文' : 'Switch to English'"
        >
          <span :class="{ active: language === 'en' }">EN</span><i>/</i
          ><span :class="{ active: language === 'zh-CN' }">中文</span>
        </button>
      </nav>
      <a class="nav-cta" href="#download"
        >{{ t("Get connected") }} <span>↗</span></a
      >
    </header>

    <main>
      <section class="hero" aria-labelledby="hero-title">
        <div class="hero-art">
          <ConnectionField />
          <div class="horizon-ring ring-one"></div>
          <div class="horizon-ring ring-two"></div>
          <div class="horizon-ring ring-three"></div>
        </div>
        <div class="hero-shade"></div>
        <div class="hero-copy">
          <a
            class="release-pill"
            href="https://github.com/myki-jim/frp-sh/releases/tag/v0.4.0"
            ><i></i> {{ t("INTRODUCING FRP-SH 0.4") }} <span>↗</span></a
          >
          <h1 id="hero-title">
            <span class="hero-line"
              ><span>{{ t("Less distance.") }}</span></span
            ><span class="hero-line"
              ><span class="blue-text">{{ t("More connection.") }}</span></span
            >
          </h1>
          <p class="hero-sub">
            {{ t("Your machines. Your people. One room away.") }}<br />{{
              t(
                "A lean P2P tunnel built to get you connected—and out of your way.",
              )
            }}
          </p>
          <div class="hero-actions">
            <a class="button primary" href="#download"
              >{{ t("Start connecting") }} <span>↗</span></a
            ><a class="text-link" href="#experience"
              >{{ t("Explore the connection") }} <span>↓</span></a
            >
          </div>
        </div>
        <div class="hero-bottom">
          <span>{{ t("BUILT IN RUST. MADE FOR REAL CONNECTIONS.") }}</span
          ><span class="scroll-label"
            >{{ t("SCROLL TO DISCOVER") }} <b>↓</b></span
          >
        </div>
        <div class="image-label left-label"><i></i> {{ t("YOUR DEVICE") }}</div>
        <div class="image-label right-label"><i></i> {{ t("YOUR WORLD") }}</div>
      </section>

      <section id="experience" class="manifesto section-pad">
        <div class="eyebrow reveal">{{ t("01 / CLOSER BY DESIGN") }}</div>
        <h2 class="manifesto-fill">
          {{ t("The best connection") }}<br />{{ t("is the one you") }}<br />{{
            t("don’t have to think about.")
          }}
        </h2>
        <div class="manifesto-bottom reveal">
          <span class="little-cross">+</span>
          <p>
            {{ t("No port-forwarding ritual. No panel to babysit.") }}<br />{{
              t("Create a room. Share the code. Let the connection follow.")
            }}
          </p>
        </div>
      </section>

      <section class="portal section-pad" aria-labelledby="direct-title">
        <div class="section-heading reveal">
          <div>
            <span class="eyebrow">{{ t("02 / TAKE THE DIRECT ROUTE") }}</span>
            <h2 id="direct-title">
              {{ t("A shorter path.") }}<br /><span class="muted">{{
                t("A faster way in.")
              }}</span>
            </h2>
          </div>
          <p>
            {{ t("Parallel address discovery finds a way through.") }}<br />{{
              t("Direct UDP comes first. If your network says no,")
            }}<br />{{ t("automatic relay fallback keeps you moving.") }}
          </p>
        </div>
        <div class="portal-art">
          <div class="portal-grid"></div>
          <div class="orb orb-one"></div>
          <div class="orb orb-two"></div>
          <svg
            viewBox="0 0 1000 330"
            role="img"
            :aria-label="t('Two devices connected by a direct path')"
          >
            <defs>
              <linearGradient id="beam">
                <stop stop-color="#2369ff" />
                <stop offset=".5" stop-color="#b3e8ff" />
                <stop offset="1" stop-color="#2369ff" />
              </linearGradient>
            </defs>
            <path class="relay-path" d="M130 165 Q500 -100 870 165" />
            <path class="connection-path" d="M130 165H870" />
            <circle class="signal-dot" cx="130" cy="165" r="5" fill="white" />
            <circle cx="130" cy="165" r="30" class="node" />
            <circle cx="870" cy="165" r="30" class="node" />
            <text x="130" y="170">A</text>
            <text x="870" y="170">B</text>
          </svg>
          <div class="path-caption">
            <span>{{ t("01 — DISCOVER") }}</span
            ><strong><i></i> {{ t("DIRECT BY DEFAULT") }}</strong
            ><span>{{ t("02 — CONNECT") }}</span>
          </div>
          <span class="diagram-note">{{ t("CONNECTION VISUALIZATION") }}</span>
        </div>
        <div class="route-features reveal">
          <div>
            <b>{{ t("Parallel discovery.") }}</b>
            <p>{{ t("Less waiting on an unresponsive path.") }}</p>
          </div>
          <div>
            <b>{{ t("Automatic fallback.") }}</b>
            <p>{{ t("TURN or TCP relay when direct isn’t available.") }}</p>
          </div>
          <div>
            <b>{{ t("Ready to reconnect.") }}</b>
            <p>{{ t("Saved profiles. Fewer repeated steps.") }}</p>
          </div>
        </div>
      </section>

      <section class="story" :aria-label="t('Connection use cases')">
        <div class="story-track">
          <article class="story-panel story-intro">
            <span class="eyebrow">{{
              t("03 / ONE ROOM. MORE POSSIBILITIES.")
            }}</span>
            <h2>
              {{ t("Same room.") }}<br />{{ t("Different") }}<br /><span
                class="blue-text"
                >{{ t("worlds.") }}</span
              >
            </h2>
            <p>
              {{ t("A connection that fits what you do.") }}<br />{{
                t("Keep scrolling to find your way in.")
              }}
            </p>
            <span class="story-arrow">⟶</span>
          </article>
          <article class="story-panel">
            <div class="scene scene-code">
              <div class="code-window">
                <div class="window-dots">
                  <i></i><i></i><i></i><span>{{ t("your next idea") }}</span>
                </div>
                <pre><span class="code-muted">{{ t("// great things start locally") }}</span>
<span class="code-blue">const</span> idea = <span class="code-green">'something new'</span>

localhost:<span class="code-green">3000</span>
<span class="code-muted">         {{ t("↓ share a room") }}</span>
your teammate<span class="code-blue">.connected</span></pre>
                <div class="code-status">
                  <i></i> {{ t("LOCAL WORK. SHARED MOMENT.") }}
                </div>
              </div>
            </div>
            <div class="story-copy">
              <span>{{ t("01 / BUILD TOGETHER") }}</span>
              <h3>
                {{ t("Localhost.") }}<br />{{ t("Meet the rest of us.") }}
              </h3>
              <p>
                {{ t("Bring a local service to your collaborators.") }}<br />{{
                  t("Less setup between an idea and a second pair of eyes.")
                }}
              </p>
            </div>
          </article>
          <article class="story-panel">
            <div class="scene scene-play">
              <div class="game-orbit orbit-a"></div>
              <div class="game-orbit orbit-b"></div>
              <div class="game-core">
                {{ t("PLAY") }}<span>{{ t("TOGETHER") }}</span>
              </div>
              <span class="player player-one">{{ t("YOU") }} <i></i></span
              ><span class="player player-two"
                ><i></i> {{ t("PLAYER 02") }}</span
              >
            </div>
            <div class="story-copy">
              <span>{{ t("02 / BRING YOUR PEOPLE") }}</span>
              <h3>
                {{ t("Different places.") }}<br />{{ t("Same game night.") }}
              </h3>
              <p>
                {{ t("Create a virtual LAN for your group.") }}<br />{{
                  t("The room code is the invitation.")
                }}
              </p>
            </div>
          </article>
          <article class="story-panel">
            <div class="scene scene-terminal">
              <div class="terminal-demo">
                <span class="terminal-label">{{
                  t("frp-sh / CONNECTION EXAMPLE")
                }}</span>
                <p><em>❯</em> frp-sh</p>
                <p class="code-muted">
                  {{ t("Room ready. Share your code.") }}
                </p>
                <div class="room-code">orbit-2048</div>
                <p class="code-green">{{ t("✓ Peer connected") }}</p>
                <p class="code-muted">{{ t("P2P tunnel · direct route") }}</p>
              </div>
            </div>
            <div class="story-copy">
              <span>{{ t("03 / STAY IN YOUR FLOW") }}</span>
              <h3>
                {{ t("All connection.") }}<br />{{ t("No extra ceremony.") }}
              </h3>
              <p>
                {{ t("A focused terminal. Clear status.") }}<br />{{
                  t("Diagnostics in their own log, not in your way.")
                }}
              </p>
            </div>
          </article>
        </div>
      </section>

      <section class="essentials section-pad">
        <div class="section-heading reveal">
          <div>
            <span class="eyebrow">{{ t("04 / LESS, BUT BETTER") }}</span>
            <h2>
              {{ t("Powerful underneath.") }}<br /><span class="muted">{{
                t("Light everywhere else.")
              }}</span>
            </h2>
          </div>
        </div>
        <div class="spec-grid">
          <article class="spec reveal">
            <span class="spec-index">01</span>
            <div class="spec-number">38.7<span>%</span></div>
            <h3>{{ t("Less binary. More possibility.") }}</h3>
            <p>
              {{
                t(
                  "Smaller combined client and helper on Windows versus the previous main binary. Same purpose, less baggage.",
                )
              }}
            </p>
            <small>{{
              t(
                "Measured Release builds. Excludes Wintun, configuration and backups.",
              )
            }}</small>
          </article>
          <article class="spec reveal">
            <span class="spec-index">02</span>
            <div class="spec-symbol">⌘</div>
            <h3>{{ t("Install once. Stay in your flow.") }}</h3>
            <p>
              {{
                t(
                  "Authorize the network helper during installation. Run everyday connections from your ordinary account.",
                )
              }}
            </p>
            <small>{{
              t("Updates use the installer. No repeated runtime elevation.")
            }}</small>
          </article>
          <article class="spec reveal">
            <span class="spec-index">03</span>
            <div class="spec-symbol shield">◇</div>
            <h3>{{ t("Your tunnel. Your keys.") }}</h3>
            <p>
              {{
                t(
                  "Enable end-to-end encryption with a matching shared key. A restricted helper keeps network privileges contained.",
                )
              }}
            </p>
            <a :href="docLink('architecture')">{{
              t("Explore the architecture ↗")
            }}</a>
          </article>
        </div>
      </section>

      <section id="download" class="download section-pad">
        <div class="download-glow"></div>
        <span class="eyebrow reveal">{{
          t("THE NEXT CONNECTION IS YOURS.")
        }}</span>
        <h2 class="reveal">{{ t("Close the distance.") }}</h2>
        <p class="reveal">
          {{ t("Open source. Native performance. Ready when you are.") }}
        </p>
        <div class="install-box reveal">
          <div
            class="platform-tabs"
            role="group"
            :aria-label="t('Installation platform')"
          >
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
          <div class="command-row">
            <span class="prompt-mark">❯</span><code>{{ command() }}</code
            ><button
              class="copy-button"
              @click="copyCommand"
              :aria-label="
                copied ? t('Command copied') : t('Copy install command')
              "
            >
              {{ copied ? t("Copied ✓") : t("Copy ↗") }}
            </button>
          </div>
          <span class="copy-status" role="status">{{
            copyError
              ? t("Clipboard unavailable. Select and copy the command above.")
              : copied
                ? t("Install command copied.")
                : ""
          }}</span>
        </div>
        <div class="download-links">
          <a :href="docLink('install')">{{ t("Read the install guide ↗") }}</a
          ><a href="https://github.com/myki-jim/frp-sh/releases/tag/v0.4.0">{{
            t("All downloads ↗")
          }}</a>
        </div>
        <p class="install-note">
          {{
            t(
              "Installation configures a privileged network helper. Review the script before running.",
            )
          }}<br />{{ t("After installation, run") }} <code>frp-sh</code>
          {{ t("to configure your signaling server.") }}
        </p>
      </section>
    </main>
    <footer class="site-footer">
      <a class="wordmark" href="/">⌁ frp.sh</a
      ><span>{{ t("LESS DISTANCE. MORE CONNECTION.") }}</span>
      <div>
        <a href="https://github.com/myki-jim/frp-sh">GitHub ↗</a
        ><a :href="docLink('versioning')">{{ t("0.x version policy") }}</a
        ><span>{{ t("MIT LICENSE") }}</span>
      </div>
    </footer>
  </div>
</template>
