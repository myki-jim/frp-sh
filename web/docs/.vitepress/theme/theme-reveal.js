import { nextTick } from "vue";
import { gsap } from "gsap";
import "./theme-reveal.css";

// Native snapshots preserve WebGL, scroll position and text; GSAP drives the mask.
export function createThemeReveal() {
  let active,
    tween,
    disposed = false;
  const html = document.documentElement;
  function cleanup() {
    html.classList.remove("theme-revealing");
    ["--theme-x", "--theme-y", "--theme-radius"].forEach((key) =>
      html.style.removeProperty(key),
    );
  }
  async function reveal(event, update) {
    if (disposed || active) return;
    if (
      !document.startViewTransition ||
      matchMedia("(prefers-reduced-motion: reduce)").matches ||
      document.hidden
    ) {
      update();
      return;
    }
    const rect = event?.currentTarget?.getBoundingClientRect();
    const x = rect ? rect.left + rect.width / 2 : innerWidth - 40;
    const y = rect ? rect.top + rect.height / 2 : 40;
    const radius =
      Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y)) +
      20;
    html.style.setProperty("--theme-x", `${x}px`);
    html.style.setProperty("--theme-y", `${y}px`);
    html.style.setProperty("--theme-radius", "0px");
    html.classList.add("theme-revealing");
    const transition = document.startViewTransition(async () => {
      update();
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
      const mask = { radius: 0 };
      tween = gsap.to(mask, {
        radius,
        duration: 1.1,
        ease: "expo.inOut",
        onUpdate: () =>
          html.style.setProperty("--theme-radius", `${mask.radius}px`),
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
