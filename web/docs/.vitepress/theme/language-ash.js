import { nextTick } from "vue";
import { gsap } from "gsap";
let running = false;
function glyphs(width, height) {
  const raster = document.createElement("canvas");
  raster.width = width;
  raster.height = height;
  const ctx = raster.getContext("2d", { willReadFrequently: true });
  const walker = document.createTreeWalker(
    document.querySelector(".site, .doc-shell"),
    NodeFilter.SHOW_TEXT,
  );
  const range = document.createRange();
  let node,
    count = 0;
  while ((node = walker.nextNode()) && count < 4000) {
    if (
      !node.textContent.trim() ||
      node.parentElement.closest(
        "script, style, svg, .mermaid, [aria-hidden=true]",
      )
    )
      continue;
    const style = getComputedStyle(node.parentElement);
    if (
      style.visibility === "hidden" ||
      style.display === "none" ||
      Number(style.opacity) === 0
    )
      continue;
    const bounds = node.parentElement.getBoundingClientRect();
    if (bounds.bottom < 0 || bounds.top > height) continue;
    ctx.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    ctx.fillStyle = style.color;
    ctx.textBaseline = "top";
    for (let i = 0; i < node.length && count < 4000; i++) {
      if (!node.textContent[i].trim()) continue;
      range.setStart(node, i);
      range.setEnd(node, i + 1);
      const rect = range.getBoundingClientRect();
      if (
        !rect.width ||
        rect.bottom < 0 ||
        rect.top > height ||
        rect.right < 0 ||
        rect.left > width
      )
        continue;
      ctx.fillText(node.textContent[i], rect.left, rect.top);
      count++;
    }
  }
  const data = ctx.getImageData(0, 0, width, height).data;
  const points = [];
  for (let y = 0; y < height; y += 2)
    for (let x = 0; x < width; x += 2) {
      const i = (y * width + x) * 4;
      if (data[i + 3] > 65)
        points.push({
          x,
          y,
          color: `rgb(${data[i]},${data[i + 1]},${data[i + 2]})`,
        });
    }
  const step = Math.max(1, Math.ceil(points.length / 7000));
  return points
    .filter((_, i) => i % step === 0)
    .sort((a, b) => a.x / width + a.y / height - b.x / width - b.y / height);
}

export async function dissolveLanguage(update) {
  if (running) return;
  if (
    matchMedia("(prefers-reduced-motion: reduce)").matches ||
    document.hidden
  ) {
    await update();
    return;
  }
  running = true;
  const html = document.documentElement;
  let canvas, tween;
  const width = innerWidth,
    height = innerHeight;
  let scrollStart = scrollY;
  const nodes = () =>
    [
      ...document.querySelectorAll(
        ".hero-copy h1, .hero-copy .intro, .eyebrow, .site h2, .site h3, .site p, .vp-doc h1, .vp-doc h2, .vp-doc h3, .vp-doc p, .vp-doc li, .docs-eyebrow",
      ),
    ].filter((el) => {
      const r = el.getBoundingClientRect();
      return (
        r.bottom > 0 && r.top < innerHeight && !el.parentElement.closest("li")
      );
    });
  let elements = [];
  const saved = new Map();
  function restore() {
    saved.forEach((value, el) => {
      el.style.maskImage = value;
    });
    saved.clear();
  }
  function prepare() {
    elements = nodes();
    elements.forEach((el) => saved.set(el, el.style.maskImage));
  }
  try {
    let points = glyphs(width, height);
    canvas = document.createElement("canvas");
    canvas.className = "language-dust";
    canvas.width = width;
    canvas.height = height;
    canvas.setAttribute("aria-hidden", "true");
    document.body.appendChild(canvas);
    const ctx = canvas.getContext("2d");
    prepare();
    const animate = (incoming) =>
      new Promise((resolve) => {
        const phase = { value: 0 };
        tween = gsap.to(phase, {
          value: 1,
          duration: incoming ? 0.8 : 0.72,
          ease: "sine.inOut",
          onUpdate: () => {
            const front = phase.value * 1.5 - 0.25;
            elements.forEach((el) => {
              const rect = el.getBoundingClientRect();
              const start = (rect.left / width + rect.top / height) / 2;
              const extent = Math.max(
                0.03,
                (rect.width / width + rect.height / height) / 2,
              );
              const edge = ((front - start) / extent) * 100;
              el.style.maskImage = incoming
                ? `linear-gradient(135deg,#000 ${edge - 10}%,transparent ${edge + 10}%)`
                : `linear-gradient(135deg,transparent ${edge - 10}%,#000 ${edge + 10}%)`;
            });
            ctx.clearRect(0, 0, width, height);
            points.forEach((p, i) => {
              const position = (p.x / width + p.y / height) / 2;
              const age = incoming
                ? (position - front) / 0.24
                : (front - position) / 0.24;
              if (age < 0 || age > 1) return;
              const drift = Math.sin(i * 2.399) * 22;
              ctx.globalAlpha = Math.sin(age * Math.PI) * 0.85;
              ctx.fillStyle = p.color;
              ctx.fillRect(
                p.x + age * (28 + drift),
                p.y + age * (-18 + Math.cos(i) * 24) - (scrollY - scrollStart),
                1.3,
                1.3,
              );
            });
            ctx.globalAlpha = 1;
          },
          onComplete: resolve,
        });
      });
    await animate(false);
    html.classList.add("language-ashing");
    await update();
    await nextTick();
    restore();
    points = glyphs(width, height);
    scrollStart = scrollY;
    prepare();
    elements.forEach(
      (el) => (el.style.maskImage = "linear-gradient(transparent,transparent)"),
    );
    await animate(true);
  } finally {
    tween?.kill();
    restore();
    canvas?.remove();
    html.classList.remove("language-ashing");
    running = false;
  }
}
