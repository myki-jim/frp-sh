import { nextTick } from "vue";
import { gsap } from "gsap";
import "./theme-reveal.css";
import { dissolveLanguage } from "./language-ash.js";

// Native snapshots preserve WebGL, scroll position and text; GSAP drives the mask.
export function createThemeReveal() {
  let active,
    tween,
    disposed = false;
  const html = document.documentElement;
  function cleanup() {
    html.classList.remove("theme-revealing", "language-revealing");
    [
      "--theme-x",
      "--theme-y",
      "--theme-radius",
      "--particle-mask",
      "--particle-haze",
    ].forEach((key) => html.style.removeProperty(key));
  }
  async function reveal(event, update, kind = "theme") {
    if (disposed || active || html.classList.contains("language-ashing"))
      return;
    if (kind === "language") return dissolveLanguage(update);
    if (
      !document.startViewTransition ||
      matchMedia("(prefers-reduced-motion: reduce)").matches ||
      document.hidden
    ) {
      await update();
      return;
    }
    const rect = event?.currentTarget?.getBoundingClientRect();
    const x = rect ? rect.left + rect.width / 2 : innerWidth - 40;
    const y = rect ? rect.top + rect.height / 2 : 40;
    const radius =
      Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y)) +
      180;
    html.style.setProperty("--theme-x", `${x}px`);
    html.style.setProperty("--theme-y", `${y}px`);
    html.style.setProperty("--theme-radius", "0px");
    const width = innerWidth,
      height = innerHeight;
    // Seeded particles retain their identity as the wave expands.
    const particles = Array.from(
      { length: width < 600 ? 90 : 160 },
      (_, i) => ({
        angle: i * 2.399963,
        offset: Math.sin(i * 12.9898) * 65,
        size: 1.5 + (i % 5) * 1.05,
      }),
    );
    function paint(progress) {
      const r = progress * radius;
      const amplitude = Math.sin(progress * Math.PI) * 22;
      const points = Array.from({ length: 100 }, (_, i) => {
        const a = (i / 100) * Math.PI * 2;
        const wave =
          Math.sin(a * 7 + progress * 18) +
          Math.sin(a * 13 - progress * 11) * 0.4;
        const edge = Math.max(0, r + wave * amplitude);
        return `${(x + Math.cos(a) * edge).toFixed(1)},${(y + Math.sin(a) * edge).toFixed(1)}`;
      });
      const dust = particles
        .map((p) => {
          const a = p.angle + progress * 0.25;
          const distance = Math.max(0, r + p.offset + 28);
          return `<circle cx="${(x + Math.cos(a) * distance).toFixed(1)}" cy="${(y + Math.sin(a) * distance).toFixed(1)}" r="${p.size}" opacity="${Math.sin(progress * Math.PI).toFixed(2)}"/>`;
        })
        .join("");
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"><defs><filter id="soft" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="9"/></filter></defs><g fill="white"><polygon points="${points.join(" ")}" filter="url(#soft)"/>${dust}</g></svg>`;
      html.style.setProperty(
        "--particle-mask",
        `url("data:image/svg+xml,${encodeURIComponent(svg)}")`,
      );
      html.style.setProperty("--theme-radius", `${r}px`);
      html.style.setProperty(
        "--particle-haze",
        `${Math.sin(progress * Math.PI) * 2}px`,
      );
    }
    paint(0);
    html.classList.add("theme-revealing");
    if (kind === "language") html.classList.add("language-revealing");
    const transition = document.startViewTransition(async () => {
      await update();
      await nextTick();
      // Rendering is paused during snapshot updates; do not await an animation frame.
    });
    active = transition;
    try {
      await transition.ready;
      if (disposed) {
        transition.skipTransition();
        return;
      }
      const mask = { progress: 0 };
      tween = gsap.to(mask, {
        progress: 1,
        duration: 1.45,
        ease: "expo.inOut",
        onUpdate: () => paint(mask.progress),
        onComplete: () => transition.skipTransition(),
      });
      await transition.finished;
    } catch {
      transition.skipTransition();
      await transition.updateCallbackDone.catch(() => {});
    } finally {
      tween?.kill();
      tween = undefined;
      active = undefined;
      cleanup();
    }
  }
  return {
    reveal,
    dispose() {
      disposed = true;
      tween?.kill();
      active?.skipTransition();
      cleanup();
    },
  };
}
