import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

export function mountDocsMotion(shell) {
  gsap.registerPlugin(ScrollTrigger);
  const mm = gsap.matchMedia();
  mm.add("(prefers-reduced-motion: no-preference)", () => {
    gsap.from(shell.querySelectorAll(".docs-eyebrow, .vp-doc h1"), {
      y: 38,
      opacity: 0,
      duration: 0.75,
      stagger: 0.09,
      ease: "power3.out",
      clearProps: "transform,opacity",
    });
    shell
      .querySelectorAll(
        '.vp-doc h2, .vp-doc div[class*="language-"], .vp-doc .custom-block, .vp-doc .mermaid',
      )
      .forEach((el) => {
        gsap.from(el, {
          y: 26,
          opacity: 0.55,
          duration: 0.7,
          ease: "power3.out",
          clearProps: "transform,opacity",
          scrollTrigger: { trigger: el, start: "top 96%", once: true },
        });
      });
    gsap.fromTo(
      shell.querySelector(".docs-reading-progress"),
      { scaleX: 0 },
      {
        scaleX: 1,
        ease: "none",
        scrollTrigger: {
          trigger: shell.querySelector(".VPDoc"),
          start: "top top",
          end: "bottom bottom",
          scrub: 0.2,
        },
      },
    );
  });
  let frame;
  const observer = new ResizeObserver(() => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => ScrollTrigger.refresh());
  });
  const article = shell.querySelector(".vp-doc");
  if (article) observer.observe(article);
  return () => {
    cancelAnimationFrame(frame);
    observer.disconnect();
    mm.revert();
  };
}
