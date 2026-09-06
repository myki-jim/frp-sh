import { nextTick } from "vue";
import { gsap } from "gsap";
let running = false;
function visibleText() {
  return [
    ...document.querySelectorAll(
      ".hero-copy h1, .hero-copy .intro, .eyebrow, .site h2, .site h3, .connection-copy > p, .use-grid p, .details p, .vp-doc h1, .vp-doc h2, .vp-doc h3, .vp-doc p, .vp-doc li, .docs-eyebrow",
    ),
  ].filter((el) => {
    const r = el.getBoundingClientRect();
    return (
      r.bottom > 0 && r.top < innerHeight && !el.parentElement.closest("li")
    );
  });
}
// Animate native glyphs in place: no raster sampling, hidden page, or frozen overlay.
export async function dissolveLanguage(update) {
  if (running) return;
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    await update();
    return;
  }
  running = true;
  let old = [],
    fresh = [];
  const diagonal = (_, el) => {
    const r = el.getBoundingClientRect();
    return Math.max(0, (r.left / innerWidth + r.top / innerHeight) * 0.1);
  };
  try {
    old = visibleText();
    await gsap.to(old, {
      opacity: 0.35,
      filter: "blur(2px)",
      duration: 0.2,
      stagger: diagonal,
      ease: "sine.inOut",
    });
    gsap.set(old, { clearProps: "opacity,filter" });
    document.documentElement.classList.add("language-ashing");
    await update();
    await nextTick();
    fresh = visibleText();
    await gsap.fromTo(
      fresh,
      { opacity: 0.35, filter: "blur(3px)" },
      {
        opacity: 1,
        filter: "blur(0px)",
        duration: 0.5,
        stagger: diagonal,
        ease: "power2.out",
        clearProps: "opacity,filter",
      },
    );
  } finally {
    gsap.set([...old, ...fresh], { clearProps: "opacity,filter" });
    document.documentElement.classList.remove("language-ashing");
    running = false;
  }
}
