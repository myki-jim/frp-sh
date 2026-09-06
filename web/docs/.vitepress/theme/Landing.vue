<script setup>
import { ref, onMounted, onBeforeUnmount } from "vue";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import "./landing.css";

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
  gsap.registerPlugin(ScrollTrigger);
  mm = gsap.matchMedia();
  mm.add(
    "(prefers-reduced-motion: no-preference)",
    () => {
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
      gsap.utils
        .toArray(".reveal")
        .forEach((el) =>
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
  <div ref="root" class="landing">
    <a class="skip-link" href="#experience">Skip to content</a>
    <header class="site-header">
      <a class="wordmark" href="/" aria-label="frp-sh home"
        ><span class="brand-glyph" aria-hidden="true">⌁</span> frp<span
          class="brand-dot"
          >.</span
        >sh</a
      >
      <nav aria-label="Main navigation">
        <a href="#experience">Experience</a
        ><a href="/en/quickstart">Documentation ↗</a
        ><a href="/intro" lang="zh-CN">中文</a>
      </nav>
      <a class="nav-cta" href="#download">Get connected <span>↗</span></a>
    </header>

    <main>
      <section class="hero" aria-labelledby="hero-title">
        <img
          class="hero-art"
          src="/images/connection-beam.png"
          width="1536"
          height="1024"
          alt=""
          fetchpriority="high"
        />
        <div class="hero-shade"></div>
        <div class="hero-copy">
          <a
            class="release-pill"
            href="https://github.com/myki-jim/frp-sh/releases/tag/v0.4.0"
            ><i></i> INTRODUCING FRP-SH 0.4 <span>↗</span></a
          >
          <h1 id="hero-title">
            <span class="hero-line"><span>Less distance.</span></span
            ><span class="hero-line"
              ><span class="blue-text">More connection.</span></span
            >
          </h1>
          <p class="hero-sub">
            Your machines. Your people. One room away.<br />A lean P2P tunnel
            built to get you connected—and out of your way.
          </p>
          <div class="hero-actions">
            <a class="button primary" href="#download"
              >Start connecting <span>↗</span></a
            ><a class="text-link" href="#experience"
              >Explore the connection <span>↓</span></a
            >
          </div>
        </div>
        <div class="hero-bottom">
          <span>BUILT IN RUST. MADE FOR REAL CONNECTIONS.</span
          ><span class="scroll-label">SCROLL TO DISCOVER <b>↓</b></span>
        </div>
        <div class="image-label left-label"><i></i> YOUR DEVICE</div>
        <div class="image-label right-label"><i></i> YOUR WORLD</div>
      </section>

      <section id="experience" class="manifesto section-pad">
        <div class="eyebrow reveal">01 / CLOSER BY DESIGN</div>
        <h2 class="manifesto-fill">
          The best connection<br />is the one you<br />don’t have to think
          about.
        </h2>
        <div class="manifesto-bottom reveal">
          <span class="little-cross">+</span>
          <p>
            No port-forwarding ritual. No panel to babysit.<br />Create a room.
            Share the code. Let the connection follow.
          </p>
        </div>
      </section>

      <section class="portal section-pad" aria-labelledby="direct-title">
        <div class="section-heading reveal">
          <div>
            <span class="eyebrow">02 / TAKE THE DIRECT ROUTE</span>
            <h2 id="direct-title">
              A shorter path.<br /><span class="muted">A faster way in.</span>
            </h2>
          </div>
          <p>
            Parallel address discovery finds a way through.<br />Direct UDP
            comes first. If your network says no,<br />automatic relay fallback
            keeps you moving.
          </p>
        </div>
        <div class="portal-art">
          <div class="portal-grid"></div>
          <div class="orb orb-one"></div>
          <div class="orb orb-two"></div>
          <svg
            viewBox="0 0 1000 330"
            role="img"
            aria-label="Two devices connected by a direct path"
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
            <span>01 — DISCOVER</span><strong><i></i> DIRECT BY DEFAULT</strong
            ><span>02 — CONNECT</span>
          </div>
          <span class="diagram-note">CONNECTION VISUALIZATION</span>
        </div>
        <div class="route-features reveal">
          <div>
            <b>Parallel discovery.</b>
            <p>Less waiting on an unresponsive path.</p>
          </div>
          <div>
            <b>Automatic fallback.</b>
            <p>TURN or TCP relay when direct isn’t available.</p>
          </div>
          <div>
            <b>Ready to reconnect.</b>
            <p>Saved profiles. Fewer repeated steps.</p>
          </div>
        </div>
      </section>

      <section class="story" aria-label="Connection use cases">
        <div class="story-track">
          <article class="story-panel story-intro">
            <span class="eyebrow">03 / ONE ROOM. MORE POSSIBILITIES.</span>
            <h2>
              Same room.<br />Different<br /><span class="blue-text"
                >worlds.</span
              >
            </h2>
            <p>
              A connection that fits what you do.<br />Keep scrolling to find
              your way in.
            </p>
            <span class="story-arrow">⟶</span>
          </article>
          <article class="story-panel">
            <div class="scene scene-code">
              <div class="code-window">
                <div class="window-dots">
                  <i></i><i></i><i></i><span>your next idea</span>
                </div>
                <pre><span class="code-muted">// great things start locally</span>
<span class="code-blue">const</span> idea = <span class="code-green">'something new'</span>

localhost:<span class="code-green">3000</span>
<span class="code-muted">         ↓ share a room</span>
your teammate<span class="code-blue">.connected</span></pre>
                <div class="code-status">
                  <i></i> LOCAL WORK. SHARED MOMENT.
                </div>
              </div>
            </div>
            <div class="story-copy">
              <span>01 / BUILD TOGETHER</span>
              <h3>Localhost.<br />Meet the rest of us.</h3>
              <p>
                Bring a local service to your collaborators.<br />Less setup
                between an idea and a second pair of eyes.
              </p>
            </div>
          </article>
          <article class="story-panel">
            <div class="scene scene-play">
              <div class="game-orbit orbit-a"></div>
              <div class="game-orbit orbit-b"></div>
              <div class="game-core">PLAY<span>TOGETHER</span></div>
              <span class="player player-one">YOU <i></i></span
              ><span class="player player-two"><i></i> PLAYER 02</span>
            </div>
            <div class="story-copy">
              <span>02 / BRING YOUR PEOPLE</span>
              <h3>Different places.<br />Same game night.</h3>
              <p>
                Create a virtual LAN for your group.<br />The room code is the
                invitation.
              </p>
            </div>
          </article>
          <article class="story-panel">
            <div class="scene scene-terminal">
              <div class="terminal-demo">
                <span class="terminal-label">frp-sh / CONNECTION EXAMPLE</span>
                <p><em>❯</em> frp-sh</p>
                <p class="code-muted">Room ready. Share your code.</p>
                <div class="room-code">orbit-2048</div>
                <p class="code-green">✓ Peer connected</p>
                <p class="code-muted">P2P tunnel · direct route</p>
              </div>
            </div>
            <div class="story-copy">
              <span>03 / STAY IN YOUR FLOW</span>
              <h3>All connection.<br />No extra ceremony.</h3>
              <p>
                A focused terminal. Clear status.<br />Diagnostics in their own
                log, not in your way.
              </p>
            </div>
          </article>
        </div>
      </section>

      <section class="essentials section-pad">
        <div class="section-heading reveal">
          <div>
            <span class="eyebrow">04 / LESS, BUT BETTER</span>
            <h2>
              Powerful underneath.<br /><span class="muted"
                >Light everywhere else.</span
              >
            </h2>
          </div>
        </div>
        <div class="spec-grid">
          <article class="spec reveal">
            <span class="spec-index">01</span>
            <div class="spec-number">38.7<span>%</span></div>
            <h3>Less binary. More possibility.</h3>
            <p>
              Smaller combined client and helper on Windows versus the previous
              main binary. Same purpose, less baggage.
            </p>
            <small
              >Measured Release builds. Excludes Wintun, configuration and
              backups.</small
            >
          </article>
          <article class="spec reveal">
            <span class="spec-index">02</span>
            <div class="spec-symbol">⌘</div>
            <h3>Install once. Stay in your flow.</h3>
            <p>
              Authorize the network helper during installation. Run everyday
              connections from your ordinary account.
            </p>
            <small
              >Updates use the installer. No repeated runtime elevation.</small
            >
          </article>
          <article class="spec reveal">
            <span class="spec-index">03</span>
            <div class="spec-symbol shield">◇</div>
            <h3>Your tunnel. Your keys.</h3>
            <p>
              Enable end-to-end encryption with a matching shared key. A
              restricted helper keeps network privileges contained.
            </p>
            <a href="/en/architecture">Explore the architecture ↗</a>
          </article>
        </div>
      </section>

      <section id="download" class="download section-pad">
        <div class="download-glow"></div>
        <span class="eyebrow reveal">THE NEXT CONNECTION IS YOURS.</span>
        <h2 class="reveal">Close the distance.</h2>
        <p class="reveal">
          Open source. Native performance. Ready when you are.
        </p>
        <div class="install-box reveal">
          <div
            class="platform-tabs"
            role="group"
            aria-label="Installation platform"
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
              :aria-label="copied ? 'Command copied' : 'Copy install command'"
            >
              {{ copied ? "Copied ✓" : "Copy ↗" }}
            </button>
          </div>
          <span class="copy-status" role="status">{{
            copyError
              ? "Clipboard unavailable. Select and copy the command above."
              : copied
                ? "Install command copied."
                : ""
          }}</span>
        </div>
        <div class="download-links">
          <a href="/en/install">Read the install guide ↗</a
          ><a href="https://github.com/myki-jim/frp-sh/releases/tag/v0.4.0"
            >All downloads ↗</a
          >
        </div>
        <p class="install-note">
          Installation configures a privileged network helper. Review the script
          before running.<br />After installation, run <code>frp-sh</code> to
          configure your signaling server.
        </p>
      </section>
    </main>
    <footer class="site-footer">
      <a class="wordmark" href="/">⌁ frp.sh</a
      ><span>LESS DISTANCE. MORE CONNECTION.</span>
      <div>
        <a href="https://github.com/myki-jim/frp-sh">GitHub ↗</a
        ><a href="/en/versioning">0.x version policy</a><span>MIT LICENSE</span>
      </div>
    </footer>
  </div>
</template>
