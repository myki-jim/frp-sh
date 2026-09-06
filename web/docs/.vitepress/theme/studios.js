import * as T from "three";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";

export function mountStudios(host, state, motion = { progress: 0.5 }) {
  const scene = new T.Scene(),
    camera = new T.OrthographicCamera(-10, 10, 7, -7, 0.1, 100);
  const renderer = new T.WebGLRenderer({
    alpha: true,
    antialias: true,
    powerPreference: "low-power",
  });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 1.5));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = T.PCFSoftShadowMap;
  renderer.shadowMap.autoUpdate = false;
  renderer.shadowMap.needsUpdate = true;
  renderer.toneMapping = T.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1;
  host.appendChild(renderer.domElement);
  const geometries = new Map(),
    materials = new Map(),
    extras = [],
    textures = [];
  function mat(color) {
    if (!materials.has(color))
      materials.set(
        color,
        new T.MeshStandardMaterial({ color, roughness: 0.68 }),
      );
    return materials.get(color);
  }
  function box(parent, size, pos, color, r = 0.025) {
    const k = size + "/" + r;
    if (!geometries.has(k))
      geometries.set(
        k,
        new RoundedBoxGeometry(...size, 3, Math.min(r, Math.min(...size) / 3)),
      );
    const m = new T.Mesh(geometries.get(k), mat(color));
    m.position.set(...pos);
    m.castShadow = true;
    m.receiveShadow = true;
    parent.add(m);
    return m;
  }
  function cylinder(parent, r, h, pos, color, top = r) {
    const k = `c${r}/${h}/${top}`;
    if (!geometries.has(k))
      geometries.set(k, new T.CylinderGeometry(top, r, h, 32));
    const m = new T.Mesh(geometries.get(k), mat(color));
    m.position.set(...pos);
    m.castShadow = true;
    m.receiveShadow = true;
    parent.add(m);
    return m;
  }
  function ellipsoid(parent, size, pos, color) {
    if (!geometries.has("sphere"))
      geometries.set("sphere", new T.SphereGeometry(1, 24, 16));
    const m = new T.Mesh(geometries.get("sphere"), mat(color));
    m.scale.set(...size);
    m.position.set(...pos);
    m.castShadow = true;
    parent.add(m);
    return m;
  }
  const hemi = new T.HemisphereLight(0xeaf0ff, 0xa29682, 2.1);
  scene.add(hemi);
  const sun = new T.DirectionalLight(0xffefd8, 2.6);
  sun.position.set(-8, 12, 6);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  Object.assign(sun.shadow.camera, {
    left: -14,
    right: 14,
    top: 12,
    bottom: -12,
    far: 40,
  });
  sun.shadow.normalBias = 0.025;
  scene.add(sun);
  const fill = new T.DirectionalLight(0xcbdcf4, 0.6);
  fill.position.set(9, 6, -8);
  scene.add(fill);
  const ground = box(scene, [70, 0.1, 70], [0, -0.28, 0], 0xffffff);
  ground.castShadow = false;
  const shadow = new T.ShadowMaterial({ opacity: 0.13 });
  extras.push(shadow);
  ground.material = shadow;
  const palettes = [
    [0xe7dfcf, 0xbd956e, 0x687d74],
    [0xb8c0bd, 0x656d6c, 0xb27459],
    [0xd9d2c7, 0x856951, 0x4c5e69],
    [0xbac6b4, 0xc7ae85, 0x859671],
    [0xd4c3b7, 0x956e58, 0x9c7166],
    [0xc9c9bb, 0xa79c7c, 0x687868],
  ];
  const lights = [],
    screens = [],
    groups = [];
  function plant(g, x, z, s = 1) {
    const p = new T.Group();
    g.add(p);
    p.position.set(x, 0.04, z);
    p.scale.setScalar(s);
    cylinder(p, 0.14, 0.28, [0, 0.14, 0], 0xd6cbb6, 0.18);
    for (let j = 0; j < 7; j++) {
      const a = j * 2.4;
      const leaf = ellipsoid(
        p,
        [0.09, 0.27, 0.035],
        [Math.sin(a) * 0.12, 0.43 + (j % 3) * 0.06, Math.cos(a) * 0.12],
        j % 2 ? 0x6f865d : 0x89996e,
      );
      leaf.rotation.set(Math.cos(a) * 0.45, a, Math.sin(a) * 0.5);
    }
  }
  for (let i = 0; i < 6; i++) {
    const g = new T.Group();
    g.position.set(((i % 3) - 1) * 4.65, 0, i < 3 ? 2.05 : -2.05);
    scene.add(g);
    groups.push(g);
    const [wall, wood, fabric] = palettes[i];
    box(g, [3.8, 0.18, 3.45], [0, -0.1, 0], 0xc1b8a6, 0.07);
    box(
      g,
      [3.73, 0.035, 3.38],
      [0, 0.01, 0],
      i === 1 ? 0xbcc0b7 : 0xd8cdb8,
      0.008,
    );
    for (let j = 0; j < 8; j++)
      box(
        g,
        [0.004, 0.002, 3.36],
        [-1.6 + j * 0.45, 0.03, 0],
        0xb8ac97,
        0.0005,
      );
    // A low side return and a true window opening, kept clear of all furniture.
    box(g, [0.1, 0.6, 3.35], [-1.85, 0.32, 0], wall);
    box(g, [3.75, 0.48, 0.1], [0, 0.27, -1.68], wall);
    const windowWidth = i === 3 ? 3.3 : 1.6,
      side = (3.75 - windowWidth) / 2;
    for (const sign of [-1, 1])
      box(
        g,
        [side, 1.26, 0.1],
        [sign * (windowWidth / 2 + side / 2), 1.14, -1.68],
        wall,
      );
    box(g, [3.75, 0.11, 0.1], [0, 1.81, -1.68], wall);
    for (const x of [-windowWidth / 2, 0, windowWidth / 2])
      box(g, [0.035, 1.28, 0.08], [x, 1.14, -1.65], 0xf0e8d8, 0.006);
    box(g, [windowWidth, 0.04, 0.18], [0, 0.51, -1.6], wood, 0.006);
    const glass = new T.MeshPhysicalMaterial({
      color: 0xc4dae1,
      transparent: true,
      opacity: 0.13,
      roughness: 0.15,
      depthWrite: false,
    });
    extras.push(glass);
    const pane = box(
      g,
      [windowWidth, 1.24, 0.01],
      [0, 1.14, -1.66],
      0xffffff,
      0.001,
    );
    pane.material = glass;
    pane.castShadow = false;
    const dx = i === 4 ? -0.25 : 0;
    box(g, [1.9, 0.09, 0.82], [dx, 0.89, -0.48], wood, 0.035);
    for (const x of [-0.8, 0.8])
      for (const z of [-0.78, -0.17])
        box(g, [0.045, 0.85, 0.045], [dx + x, 0.435, z], 0x444d46, 0.01);
    box(g, [0.32, 0.035, 0.23], [dx, 0.953, -0.67], 0x46514a, 0.02);
    box(g, [0.06, 0.18, 0.05], [dx, 1.05, -0.71], 0x46514a, 0.012);
    box(
      g,
      [1.04, 0.65, 0.075],
      [dx, 1.4, -0.71],
      i === 5 ? 0xc4bea8 : 0x38443d,
      0.026,
    );
    const canvas = document.createElement("canvas");
    canvas.width = 512;
    canvas.height = 300;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#172a29";
    ctx.fillRect(0, 0, 512, 300);
    ctx.fillStyle = "#9baea0";
    for (let j = 0; j < 5; j++)
      ctx.fillRect(32, 45 + j * 42, 100 + (j % 3) * 75, 10);
    ctx.fillStyle = "#af8060";
    ctx.fillRect(375, 42, 100, 205);
    const texture = new T.CanvasTexture(canvas);
    texture.colorSpace = T.SRGBColorSpace;
    textures.push(texture);
    const sm = new T.MeshBasicMaterial({ map: texture, toneMapped: false });
    extras.push(sm);
    const screen = box(
      g,
      [0.94, 0.55, 0.008],
      [dx, 1.4, -0.668],
      0xffffff,
      0.002,
    );
    screen.material = sm;
    screens.push(sm);
    box(g, [0.65, 0.025, 0.21], [dx - 0.08, 0.954, -0.24], 0xe4dfd2, 0.013);
    for (let row = 0; row < 3; row++)
      for (let col = 0; col < 9; col++)
        box(
          g,
          [0.045, 0.006, 0.043],
          [dx - 0.34 + col * 0.062, 0.97, -0.31 + row * 0.057],
          0xb8beb2,
          0.004,
        );
    ellipsoid(g, [0.055, 0.024, 0.085], [dx + 0.46, 0.961, -0.23], 0xdcdccc);
    cylinder(g, 0.065, 0.14, [dx + 0.77, 1.005, -0.48], fabric);
    // Chair has a fixed clearance from the desk; no intersecting camera or furniture animation.
    box(g, [0.58, 0.1, 0.55], [dx, 0.49, 0.63], fabric, 0.04);
    box(g, [0.58, 0.52, 0.09], [dx, 0.79, 0.88], fabric, 0.035);
    cylinder(g, 0.028, 0.39, [dx, 0.25, 0.63], 0x586157);
    for (const a of [0, Math.PI / 2]) {
      const foot = box(
        g,
        [0.61, 0.025, 0.04],
        [dx, 0.07, 0.63],
        0x596358,
        0.008,
      );
      foot.rotation.y = a;
    }
    for (const x of [-0.3, 0.3])
      box(g, [0.025, 0.24, 0.025], [dx + x, 0.55, 0.58], 0x596358, 0.006);
    box(
      g,
      [1.15, 0.012, 1.15],
      [dx, 0.04, 0.67],
      i === 4 ? 0xbc9c8b : 0xb8bda7,
      0.015,
    );
    if (i === 0) {
      box(g, [0.48, 0.8, 0.8], [-1.4, 0.43, -0.38], wood);
      for (let k = 0; k < 5; k++)
        box(
          g,
          [0.075, 0.2 + (k % 2) * 0.04, 0.18],
          [-1.56 + k * 0.085, 0.96, -0.32],
          [0xb8795b, 0x6c7b6e, 0xdacfb8][k % 3],
        );
      plant(g, 1.4, -1.17, 0.8);
    }
    if (i === 1) {
      for (const y of [0.32, 0.78, 1.24])
        box(g, [0.42, 0.04, 1.7], [1.48, y, -0.15], 0x58655f);
      for (const z of [-0.95, 0.65])
        box(g, [0.035, 1.4, 0.035], [1.48, 0.7, z], 0x58655f);
      for (let j = 0; j < 3; j++)
        box(g, [0.3, 0.2, 0.27], [1.48, 0.44, -0.67 + j * 0.42], 0xa8ac96);
    }
    if (i === 2) {
      for (const sign of [-1, 1]) {
        const beam = box(
          g,
          [2.02, 0.075, 0.1],
          [sign * 0.91, 2.14, -1.68],
          wood,
        );
        beam.rotation.z = -sign * 0.32;
      }
      box(g, [0.65, 0.04, 0.4], [1.25, 0.06, 0.85], 0x717d73);
      plant(g, -1.4, 0.96, 0.6);
    }
    if (i === 3) {
      plant(g, -1.4, -1.13, 1.25);
      plant(g, 1.36, -1.05, 1.4);
      plant(g, 1.4, 1.03, 0.7);
      for (const z of [-1.65, 1.65])
        box(g, [0.045, 1.85, 0.045], [-1.85, 0.95, z], 0x6a806a);
    }
    if (i === 4) {
      box(g, [0.69, 0.22, 1.6], [1.35, 0.16, 0.2], wood, 0.045);
      box(g, [0.7, 0.13, 1.57], [1.35, 0.335, 0.2], 0xd2c8b7, 0.05);
      box(g, [0.6, 0.1, 0.38], [1.35, 0.445, -0.3], 0xe6d8c1, 0.04);
      box(g, [0.71, 0.035, 0.8], [1.35, 0.419, 0.5], 0x8e9c90, 0.015);
    }
    if (i === 5) {
      for (let y = 0; y < 3; y++)
        for (let z = 0; z < 3; z++)
          box(
            g,
            [0.4, 0.23, 0.38],
            [-1.4, 0.17 + y * 0.25, -0.85 + z * 0.4],
            y % 2 ? 0x9da187 : 0xb4b197,
            0.012,
          );
      box(g, [0.38, 0.12, 0.24], [0.7, 1, -0.68], 0x869477, 0.02);
    }
    const lampX = i === 0 ? 1.35 : -1.35,
      lampZ = 1.1;
    cylinder(g, 0.14, 0.035, [lampX, 0.055, lampZ], 0x48564a);
    cylinder(g, 0.017, 1.12, [lampX, 0.62, lampZ], 0x48564a);
    cylinder(g, 0.21, 0.21, [lampX, 1.25, lampZ], 0xe8dcc3, 0.12);
    const light = new T.PointLight(0xffc68e, 0, 4.5, 2);
    light.position.set(lampX, 1.1, lampZ);
    g.add(light);
    lights.push(light);
  }
  const curves = [],
    dots = [];
  for (const [a, b] of [
    [0, 1],
    [1, 2],
    [0, 3],
    [1, 4],
    [2, 5],
    [3, 4],
    [4, 5],
  ]) {
    const pa = groups[a].position,
      pb = groups[b].position;
    const c = new T.CatmullRomCurve3([
      new T.Vector3(pa.x, 0.055, pa.z + 1.65),
      new T.Vector3((pa.x + pb.x) / 2, 0.055, (pa.z + pb.z) / 2 + 1.88),
      new T.Vector3(pb.x, 0.055, pb.z + 1.65),
    ]);
    curves.push(c);
    const g = new T.TubeGeometry(c, 40, 0.013, 6, false);
    geometries.set("wire" + a + b, g);
    const line = new T.Mesh(g, mat(0x9aab8a));
    scene.add(line);
    const dot = ellipsoid(scene, [0.045, 0.045, 0.045], [0, 0, 0], 0xc78350);
    dots.push(dot);
  }
  let frame = 0,
    disposed = false,
    night = state.night ? 1 : 0,
    last = 0,
    previousNight = -1;
  camera.position.set(11, 13, 18);
  camera.lookAt(0, 0.3, 0);
  function render(time = performance.now()) {
    if (disposed) return;
    const dt = Math.min(0.06, (time - last) / 1000 || 0.02);
    last = time;
    night = state.reduced
      ? Number(state.night)
      : T.MathUtils.lerp(night, Number(state.night), 1 - Math.exp(-dt * 2));
    hemi.intensity = T.MathUtils.lerp(2.1, 0.65, night);
    sun.intensity = T.MathUtils.lerp(2.6, 0.22, night);
    fill.intensity = T.MathUtils.lerp(0.6, 0.55, night);
    lights.forEach((l) => (l.intensity = night * 3.2));
    dots.forEach((dot, i) =>
      dot.position.copy(
        curves[i].getPointAt(
          state.reduced ? 0.5 : (time * 0.00035 + i * 0.15) % 1,
        ),
      ),
    );
    if (Math.abs(night - previousNight) > 0.005) {
      renderer.shadowMap.needsUpdate = true;
      previousNight = night;
    }
    // Orbit the complete arrangement; furniture and room clearances stay fixed.
    const progress = state.reduced ? 0.5 : motion.progress;
    const angle = T.MathUtils.lerp(0.15, 0.82, progress);
    camera.position.set(
      Math.sin(angle) * 22,
      T.MathUtils.lerp(16, 12, progress),
      Math.cos(angle) * 22,
    );
    camera.lookAt(0, 0.3, 0);
    camera.zoom = T.MathUtils.lerp(0.87, 1.04, Math.sin(progress * Math.PI));
    camera.updateProjectionMatrix();
    renderer.render(scene, camera);
    if (!document.hidden && !state.reduced && visible)
      frame = requestAnimationFrame(render);
  }
  function refresh() {
    cancelAnimationFrame(frame);
    render();
  }
  function resize() {
    const w = host.clientWidth,
      h = host.clientHeight;
    if (!w || !h) return;
    const half = Math.max(8.3, (5.2 * w) / h);
    camera.left = -half;
    camera.right = half;
    camera.top = (half * h) / w;
    camera.bottom = (-half * h) / w;
    camera.updateProjectionMatrix();
    renderer.setSize(w, h);
    refresh();
  }
  let visible = true;
  const observer = new ResizeObserver(resize);
  observer.observe(host);
  const io = new IntersectionObserver(([e]) => {
    visible = e.isIntersecting;
    refresh();
  });
  io.observe(host);
  document.addEventListener("visibilitychange", refresh);
  resize();
  return {
    refresh,
    dispose() {
      disposed = true;
      cancelAnimationFrame(frame);
      observer.disconnect();
      io.disconnect();
      document.removeEventListener("visibilitychange", refresh);
      geometries.forEach((g) => g.dispose());
      materials.forEach((m) => m.dispose());
      extras.forEach((m) => m.dispose());
      textures.forEach((t) => t.dispose());
      renderer.dispose();
      renderer.domElement.remove();
    },
  };
}
