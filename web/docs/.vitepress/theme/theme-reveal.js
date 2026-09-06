import { nextTick } from "vue";
import { gsap } from "gsap";
import { dissolveLanguage } from "./language-ash.js";
import "./theme-reveal.css";
export function createThemeReveal() {
  let tween,
    canvas,
    disposed = false,
    revision = 0;
  const saved = new Map();
  function remember(el) {
    if (!saved.has(el))
      saved.set(el, {
        color: el.style.color,
        backgroundColor: el.style.backgroundColor,
        backgroundImage: el.style.backgroundImage,
        backgroundAttachment: el.style.backgroundAttachment,
      });
  }
  function clear() {
    tween?.kill();
    canvas?.remove();
    saved.forEach((style, el) => Object.assign(el.style, style));
    saved.clear();
    document.documentElement.classList.remove("theme-live");
  }
  async function reveal(event, update, kind = "theme") {
    if (disposed) return;
    if (kind === "language") return dissolveLanguage(update);
    const current = ++revision;
    clear();
    if (
      matchMedia("(prefers-reduced-motion: reduce)").matches ||
      document.hidden
    ) {
      await update();
      return;
    }
    const root = document.querySelector(".site") || document.body;
    const elements = [...root.querySelectorAll("*")]
      .filter((el) => {
        if (el instanceof SVGElement || el.tagName === "CANVAS") return false;
        const r = el.getBoundingClientRect();
        return r.width && r.bottom > 0 && r.top < innerHeight;
      })
      .slice(0, 350);
    const records = elements.map((el) => {
      const css = getComputedStyle(el);
      return { el, color: css.color, bg: css.backgroundColor };
    });
    const oldBg = getComputedStyle(root).backgroundColor;
    const box = event?.currentTarget?.getBoundingClientRect();
    const x = box ? box.left + box.width / 2 : innerWidth - 30,
      y = box ? box.top + box.height / 2 : 30;
    document.documentElement.classList.add("theme-live");
    await update();
    await nextTick();
    if (disposed || current !== revision) return;
    const newBg = getComputedStyle(root).backgroundColor;
    document.documentElement.classList.add("theme-live");
    records.forEach((record) => {
      const css = getComputedStyle(record.el);
      record.colorMix = gsap.utils.interpolate(record.color, css.color);
      record.bgMix = gsap.utils.interpolate(record.bg, css.backgroundColor);
      remember(record.el);
    });
    remember(root);
    canvas = document.createElement("canvas");
    canvas.className = "theme-wave";
    canvas.setAttribute("aria-hidden", "true");
    canvas.width = innerWidth;
    canvas.height = innerHeight;
    document.body.appendChild(canvas);
    const ctx = canvas.getContext("2d");
    const max =
      Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y)) +
      110;
    const state = { progress: 0 };
    const paint = () => {
      const r = state.progress * max;
      root.style.backgroundImage = `radial-gradient(circle at ${x}px ${y}px, ${newBg} ${Math.max(0, r - 40)}px, ${oldBg} ${r + 40}px)`;
      root.style.backgroundAttachment = "fixed";
      records.forEach((record) => {
        const rect = record.el.getBoundingClientRect();
        const cx = rect.left + rect.width / 2,
          cy = rect.top + Math.min(rect.height / 2, 80);
        const a = Math.atan2(cy - y, cx - x);
        const wave = Math.sin(a * 5 + state.progress * 15) * 18;
        const t = gsap.utils.clamp(
          0,
          1,
          (r + wave - Math.hypot(cx - x, cy - y) + 60) / 120,
        );
        record.el.style.setProperty("color", record.colorMix(t), "important");
        if (record.bg !== "rgba(0, 0, 0, 0)")
          record.el.style.backgroundColor = record.bgMix(t);
      });
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      const gradient = ctx.createConicGradient(state.progress * 3, x, y);
      [
        "#65bfff",
        "#a885ff",
        "#f592ce",
        "#ffbe8b",
        "#72e0d4",
        "#65bfff",
      ].forEach((c, i) => gradient.addColorStop(i / 5, c));
      ctx.strokeStyle = gradient;
      ctx.globalAlpha = Math.sin(state.progress * Math.PI) * 0.65;
      for (let layer = 0; layer < 3; layer++) {
        ctx.beginPath();
        for (let i = 0; i <= 120; i++) {
          const a = (i / 120) * Math.PI * 2;
          const radius =
            r +
            Math.sin(a * 5 + state.progress * 15 + layer) * 18 +
            Math.sin(a * 9 - state.progress * 12) * 7;
          const px = x + Math.cos(a) * radius,
            py = y + Math.sin(a) * radius;
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.closePath();
        ctx.lineWidth = 22 - layer * 7;
        ctx.filter = `blur(${14 - layer * 5}px)`;
        ctx.stroke();
      }
    };
    paint();
    tween = gsap.to(state, {
      progress: 1,
      duration: 1.45,
      ease: "sine.inOut",
      onUpdate: paint,
      onComplete: clear,
    });
  }
  return {
    reveal,
    dispose() {
      disposed = true;
      revision++;
      clear();
    },
  };
}
