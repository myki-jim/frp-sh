import { nextTick } from "vue";
import { gsap } from "gsap";
import { dissolveLanguage } from "./language-ash.js";
import "./theme-reveal.css";

export function createThemeReveal() {
  let timeline,
    aura,
    target,
    properties = [],
    disposed = false;
  function clear() {
    timeline?.kill();
    aura?.remove();
    properties.forEach((key) => target?.style.removeProperty(key));
    target?.classList.remove("theme-live");
    timeline = aura = undefined;
    properties = [];
  }
  async function reveal(event, update, kind = "theme") {
    if (disposed) return;
    if (kind === "language") return dissolveLanguage(update);
    clear();
    if (
      matchMedia("(prefers-reduced-motion: reduce)").matches ||
      document.hidden
    ) {
      await update();
      return;
    }
    target = document.querySelector(".site") || document.documentElement;
    const site = target.matches(".site");
    properties = site
      ? ["--bg", "--ink", "--muted", "--line", "--surface", "--accent"]
      : Array.from(getComputedStyle(target)).filter(
          (key) =>
            key.startsWith("--vp-") &&
            /^(#|rgb|hsl)/.test(
              getComputedStyle(target).getPropertyValue(key).trim(),
            ),
        );
    const before = Object.fromEntries(
      properties.map((key) => [
        key,
        getComputedStyle(target).getPropertyValue(key).trim(),
      ]),
    );
    await update();
    await nextTick();
    if (disposed) return;
    const after = Object.fromEntries(
      properties.map((key) => [
        key,
        getComputedStyle(target).getPropertyValue(key).trim(),
      ]),
    );
    target.classList.add("theme-live");
    gsap.set(target, before);
    aura = document.createElement("div");
    aura.className = "theme-aura";
    aura.setAttribute("aria-hidden", "true");
    for (let i = 0; i < 3; i++) aura.appendChild(document.createElement("i"));
    document.body.appendChild(aura);
    timeline = gsap.timeline({ onComplete: clear });
    timeline.to(target, { ...after, duration: 1.15, ease: "sine.inOut" }, 0);
    timeline
      .fromTo(aura, { opacity: 0 }, { opacity: 0.36, duration: 0.3 }, 0)
      .to(aura, { opacity: 0, duration: 0.65, ease: "sine.inOut" }, 0.8);
    [...aura.children].forEach((wave, i) => {
      timeline.fromTo(
        wave,
        { rotation: i * 120, scaleX: 0.94, scaleY: 0.97 },
        {
          rotation: i * 120 + 65,
          scaleX: 1.06,
          scaleY: 1.04,
          duration: 0.7,
          yoyo: true,
          repeat: 1,
          ease: "sine.inOut",
        },
        i * 0.04,
      );
    });
  }
  return {
    reveal,
    dispose() {
      disposed = true;
      clear();
    },
  };
}
