import { nextTick } from "vue";
import { gsap } from "gsap";

let running = false;
// Sample real, currently visible glyphs rather than decorating a page-wide wipe.
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
  for (let y = 0; y < height; y += 3)
    for (let x = 0; x < width; x += 3) {
      const i = (y * width + x) * 4;
      if (data[i + 3] > 65)
        points.push({
          x,
          y,
          color: `rgb(${data[i]},${data[i + 1]},${data[i + 2]})`,
        });
    }
  const step = Math.max(1, Math.ceil(points.length / 10000));
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
  const width = innerWidth,
    height = innerHeight;
  const canvas = document.createElement("canvas");
  canvas.className = "language-ash-canvas";
  canvas.setAttribute("aria-hidden", "true");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  let tween;
  try {
    const oldPoints = glyphs(width, height);
    document.body.appendChild(canvas);
    oldPoints.forEach((p) => {
      ctx.fillStyle = p.color;
      ctx.fillRect(p.x, p.y, 2.3, 2.3);
    });
    html.classList.add("language-ashing");
    await update();
    await nextTick();
    html.classList.remove("language-ashing");
    const newPoints = glyphs(width, height);
    html.classList.add("language-ashing");
    const phase = { value: 0 };
    await new Promise((resolve) => {
      tween = gsap.to(phase, {
        value: 1,
        duration: 1.9,
        ease: "none",
        onUpdate: () => {
          ctx.clearRect(0, 0, width, height);
          const draw = (points, incoming) =>
            points.forEach((p, i) => {
              const diagonal = (p.x / width + p.y / height) / 2;
              const local = Math.max(
                0,
                Math.min(1, (phase.value - diagonal * 0.55) / 0.4),
              );
              const spread = incoming ? 1 - local : local;
              const alpha = incoming ? local : 1 - local;
              if (alpha <= 0) return;
              const drift = Math.sin(i * 2.399) * 26;
              const travel = spread * spread;
              ctx.globalAlpha = alpha;
              ctx.fillStyle = p.color;
              ctx.fillRect(
                p.x + travel * (45 + drift),
                p.y + travel * (20 + Math.cos(i) * 22),
                2.3 - spread * 0.8,
                2.3 - spread * 0.8,
              );
            });
          draw(oldPoints, false);
          draw(newPoints, true);
          ctx.globalAlpha = 1;
        },
        onComplete: resolve,
      });
    });
  } finally {
    tween?.kill();
    canvas.remove();
    html.classList.remove("language-ashing");
    running = false;
  }
}
