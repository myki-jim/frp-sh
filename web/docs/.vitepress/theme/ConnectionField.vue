<script setup>
import { ref, onMounted, onBeforeUnmount } from "vue";
const canvas = ref(null);
let dispose = () => {};
onMounted(() => {
  const el = canvas.value,
    ctx = el.getContext("2d");
  if (!ctx) return;
  const media = matchMedia("(prefers-reduced-motion: reduce)");
  let width = 0,
    height = 0,
    frame = 0,
    previous = 0,
    visible = true,
    px = 0,
    py = 0;
  const particles = Array.from({ length: 100 }, (_, i) => ({
    angle: i * 2.39996,
    depth: (i + 1) / 101,
    lane: 0.4 + (i % 7) / 10,
  }));
  function draw(time = 0) {
    const dt = Math.min((time - previous) / 1000, 0.04);
    previous = time;
    ctx.clearRect(0, 0, width, height);
    const x = width * 0.5 + px * 14,
      y = height * 0.73 + py * 9;
    const glow = ctx.createRadialGradient(x, y, 0, x, y, width * 0.55);
    glow.addColorStop(0, "#267fff45");
    glow.addColorStop(0.2, "#133cb720");
    glow.addColorStop(1, "#03060b00");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, width, height);
    for (let i = 0; i < 44; i++) {
      const a = (i / 44) * Math.PI * 2;
      ctx.beginPath();
      ctx.moveTo(x + Math.cos(a) * 32, y + Math.sin(a) * 8);
      ctx.lineTo(x + Math.cos(a) * width, y + Math.sin(a) * height * 0.65);
      ctx.strokeStyle = i % 4 === 0 ? "#418dff28" : "#456fb510";
      ctx.lineWidth = 0.6;
      ctx.stroke();
    }
    for (const p of particles) {
      if (!media.matches) p.depth = (p.depth + dt * 0.25) % 1;
      const depth = p.depth * p.depth;
      const radius = 35 + depth * width * p.lane;
      const tail = 8 + depth * 100;
      ctx.beginPath();
      ctx.moveTo(
        x + Math.cos(p.angle) * radius,
        y + Math.sin(p.angle) * radius * 0.35,
      );
      ctx.lineTo(
        x + Math.cos(p.angle) * (radius + tail),
        y + Math.sin(p.angle) * (radius + tail) * 0.35,
      );
      ctx.strokeStyle = `rgba(100,175,255,${Math.sin(p.depth * Math.PI) * 0.7})`;
      ctx.lineWidth = 0.5 + depth;
      ctx.stroke();
    }
    for (let i = 0; i < 3; i++) {
      ctx.beginPath();
      ctx.ellipse(x, y, 48 + i * 13, 13 + i * 4, 0, 0, Math.PI * 2);
      ctx.strokeStyle = ["#b9e4ff", "#67adff70", "#2e72fb30"][i];
      ctx.lineWidth = i === 0 ? 1.5 : 1;
      ctx.stroke();
    }
    if (visible && !document.hidden && !media.matches)
      frame = requestAnimationFrame(draw);
  }
  function restart() {
    cancelAnimationFrame(frame);
    previous = performance.now();
    draw(previous);
  }
  function resize() {
    const r = el.getBoundingClientRect();
    width = r.width;
    height = r.height;
    const dpr = Math.min(devicePixelRatio, 1.5);
    el.width = width * dpr;
    el.height = height * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    restart();
  }
  function pointer(e) {
    px = e.clientX / innerWidth - 0.5;
    py = e.clientY / innerHeight - 0.5;
  }
  const observer = new ResizeObserver(resize);
  observer.observe(el);
  const intersection = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    restart();
  });
  intersection.observe(el);
  document.addEventListener("visibilitychange", restart);
  media.addEventListener("change", restart);
  window.addEventListener("pointermove", pointer, { passive: true });
  dispose = () => {
    cancelAnimationFrame(frame);
    observer.disconnect();
    intersection.disconnect();
    document.removeEventListener("visibilitychange", restart);
    media.removeEventListener("change", restart);
    window.removeEventListener("pointermove", pointer);
  };
});
onBeforeUnmount(() => dispose());
</script>
<template>
  <canvas ref="canvas" class="connection-field" aria-hidden="true" />
</template>
