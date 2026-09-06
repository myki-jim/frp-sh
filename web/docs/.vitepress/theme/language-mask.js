import { nextTick } from "vue";
import { gsap } from "gsap";
let running = false;
// One viewport-wide mask. The real page stays live and receives every interaction.
export async function revealLanguage(event, update) {
  if (running) return;
  if (
    matchMedia("(prefers-reduced-motion: reduce)").matches ||
    document.hidden
  ) {
    await update();
    return;
  }
  running = true;
  const root = document.querySelector(".doc-shell, .site");
  const rect = event?.currentTarget?.getBoundingClientRect();
  const x = rect ? rect.left + rect.width / 2 : innerWidth - 35;
  const y = rect ? rect.top + rect.height / 2 : 35;
  const radius =
    Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y)) + 70;
  const layer = document.createElement("div");
  layer.className = "language-mask-layer";
  layer.setAttribute("aria-hidden", "true");
  layer.inert = true;
  const copy = root.cloneNode(true);
  const nodes = [root, ...root.querySelectorAll("*")];
  const copies = [copy, ...copy.querySelectorAll("*")];
  let tween, canvas;
  const html = document.documentElement;
  const studio = root.querySelector(".studio-shell");
  const studioZ = studio?.style.zIndex;
  const syncScroll = () => {
    copy.style.top = `${-scrollY}px`;
  };
  try {
    const rootCss = getComputedStyle(root);
    for (const key of Array.from(rootCss))
      if (
        key.startsWith("--vp-") ||
        [
          "--bg",
          "--ink",
          "--muted",
          "--line",
          "--surface",
          "--accent",
        ].includes(key)
      )
        copy.style.setProperty(key, rootCss.getPropertyValue(key));
    nodes.forEach((node, i) => {
      copies[i].removeAttribute("id");
      const r = node.getBoundingClientRect();
      if (!r.width || r.bottom < 0 || r.top > innerHeight) return;
      const css = getComputedStyle(node);
      [
        "color",
        "background-color",
        "border-top-color",
        "border-bottom-color",
        "border-left-color",
        "border-right-color",
        "box-shadow",
        "fill",
        "stroke",
      ].forEach((key) =>
        copies[i].style.setProperty(
          key,
          css.getPropertyValue(key),
          "important",
        ),
      );
    });
    Object.assign(copy.style, {
      position: "absolute",
      left: "0",
      width: `${root.getBoundingClientRect().width}px`,
      margin: "0",
      pointerEvents: "none",
    });
    layer.style.background = getComputedStyle(document.body).backgroundColor;
    if (root.matches(".site")) layer.style.background = rootCss.backgroundColor;
    layer.appendChild(copy);
    syncScroll();
    document.body.appendChild(layer);
    // Keep the original WebGL surface above the temporary text layer.
    if (studio) {
      studio.style.zIndex = "91";
      const oldStudio = copy.querySelector(".studio-shell");
      if (oldStudio) oldStudio.style.visibility = "hidden";
    }
    document.addEventListener("scroll", syncScroll, { passive: true });
    html.classList.add("language-ashing", "theme-live");
    await update();
    await nextTick();
    syncScroll();
    canvas = document.createElement("canvas");
    canvas.className = "theme-wave";
    canvas.setAttribute("aria-hidden", "true");
    canvas.width = innerWidth;
    canvas.height = innerHeight;
    document.body.appendChild(canvas);
    const ctx = canvas.getContext("2d"),
      phase = { p: 0 };
    const paint = () => {
      const r = phase.p * radius;
      layer.style.maskImage = `radial-gradient(circle at ${x}px ${y}px,transparent ${Math.max(0, r - 20)}px,#000 ${r + 20}px)`;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      const gradient = ctx.createConicGradient(phase.p * 3, x, y);
      [
        "#65bfff",
        "#a885ff",
        "#f592ce",
        "#ffbe8b",
        "#72e0d4",
        "#65bfff",
      ].forEach((c, i) => gradient.addColorStop(i / 5, c));
      ctx.strokeStyle = gradient;
      ctx.lineWidth = 18;
      ctx.filter = "blur(9px)";
      ctx.globalAlpha = Math.sin(phase.p * Math.PI) * 0.55;
      ctx.beginPath();
      for (let i = 0; i <= 120; i++) {
        const a = (i / 120) * Math.PI * 2,
          d = r + Math.sin(a * 5 + phase.p * 15) * 10;
        const px = x + Math.cos(a) * d,
          py = y + Math.sin(a) * d;
        if (!i) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.closePath();
      ctx.stroke();
    };
    paint();
    await new Promise((resolve) => {
      tween = gsap.to(phase, {
        p: 1,
        duration: 1.1,
        ease: "sine.inOut",
        onUpdate: paint,
        onComplete: resolve,
      });
    });
  } finally {
    tween?.kill();
    document.removeEventListener("scroll", syncScroll);
    layer.remove();
    canvas?.remove();
    if (studio) studio.style.zIndex = studioZ;
    html.classList.remove("language-ashing", "theme-live");
    running = false;
  }
}
