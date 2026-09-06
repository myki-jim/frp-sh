<script setup>
import { ref, watch, onMounted, onBeforeUnmount } from "vue";
import * as THREE from "three";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";
const props = defineProps({
  progress: { type: Number, default: 0 },
  reduced: Boolean,
  language: String,
});
const host = ref(null),
  failed = ref(false);
let cleanup = () => {};
onMounted(() => {
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({
      antialias: true,
      alpha: true,
      powerPreference: "low-power",
    });
  } catch {
    failed.value = true;
    return;
  }
  renderer.setPixelRatio(Math.min(devicePixelRatio, 1.5));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.4;
  host.value.appendChild(renderer.domElement);
  const scene = new THREE.Scene(),
    world = new THREE.Group();
  scene.add(world);
  const camera = new THREE.OrthographicCamera(-8, 8, 6, -6, 0.1, 100);
  camera.position.set(10, 10, 14);
  camera.lookAt(0, 0.4, 0);
  scene.add(new THREE.HemisphereLight(0xffffff, 0xb4ac98, 2.6));
  const sun = new THREE.DirectionalLight(0xfff1df, 4);
  sun.position.set(-4, 10, 7);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  Object.assign(sun.shadow.camera, {
    left: -12,
    right: 12,
    top: 10,
    bottom: -10,
    far: 35,
  });
  sun.shadow.normalBias = 0.04;
  sun.shadow.bias = -0.0001;
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0xd7e6ff, 1.5);
  fill.position.set(8, 5, -6);
  scene.add(fill);
  const materials = new Map();
  function mat(color) {
    if (!materials.has(color))
      materials.set(
        color,
        new THREE.MeshStandardMaterial({ color, roughness: 0.72 }),
      );
    return materials.get(color);
  }
  const geometries = new Set();
  function box(group, size, position, color, round = 0.025) {
    const geo = new RoundedBoxGeometry(
      ...size,
      2,
      Math.min(round, Math.min(...size) / 3),
    );
    geometries.add(geo);
    const mesh = new THREE.Mesh(geo, mat(color));
    mesh.position.set(...position);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    group.add(mesh);
    return mesh;
  }
  function cylinder(group, radius, height, position, color, top = radius) {
    const geo = new THREE.CylinderGeometry(top, radius, height, 24);
    geometries.add(geo);
    const mesh = new THREE.Mesh(geo, mat(color));
    mesh.position.set(...position);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    group.add(mesh);
    return mesh;
  }
  function room(accent, side) {
    const group = new THREE.Group();
    world.add(group);
    box(group, [4.1, 0.22, 3.8], [0, -0.12, 0], 0xd7d2c5, 0.08);
    box(group, [4, 0.06, 3.7], [0, 0.015, 0], 0xede9df);
    for (let i = 0; i < 8; i++)
      box(
        group,
        [0.009, 0.008, 3.68],
        [-1.75 + i * 0.5, 0.052, 0],
        0xdad5c9,
        0.002,
      );
    box(group, [4, 2.25, 0.13], [0, 1.15, -1.82], accent);
    group.userData.wall = box(group, [0.13, 2.25, 3.7], [-2, 1.15, 0], side ? 0xd9dfdd : 0xe7c9b9);
    // A window with a deep sill, wall art, and a small shelf.
    box(group, [1.05, 1.1, 0.065], [-0.95, 1.43, -1.73], 0xf7f1e6);
    box(
      group,
      [0.88, 0.9, 0.075],
      [-0.95, 1.43, -1.68],
      side ? 0x8ca6a1 : 0xe7a283,
    );
    box(group, [0.045, 0.95, 0.045], [-0.95, 1.43, -1.62], 0xf7f1e6);
    box(group, [1.15, 0.08, 0.3], [-0.95, 0.89, -1.6], 0xf5eee0);
    box(group, [0.56, 0.7, 0.04], [0.95, 1.67, -1.73], 0xf5efe4);
    const artwork = box(
      group,
      [0.3, 0.3, 0.045],
      [0.95, 1.67, -1.695],
      side ? 0xeb6d47 : 0x66786d,
    );
    artwork.rotation.z = 0.5;
    box(group, [1.25, 0.08, 0.36], [0.92, 0.95, -1.56], 0xb59474);
    for (let i = 0; i < 4; i++)
      box(
        group,
        [0.1, 0.26 + i * 0.03, 0.19],
        [0.55 + i * 0.13, 1.11, -1.54],
        [0xe36d4d, 0xebd9ac, 0x62736b, 0xb6c4ca][i],
      );
    // Desk, monitor, keyboard and mouse are individual modeled objects.
    box(group, [2.15, 0.12, 0.85], [0, 0.91, -0.55], 0xcfa77d, 0.06);
    for (const x of [-0.89, 0.89])
      for (const z of [-0.86, -0.25])
        box(group, [0.06, 0.85, 0.06], [x, 0.46, z], 0x454940);
    box(group, [0.35, 0.04, 0.24], [0, 1.0, -0.76], 0x424844);
    box(group, [0.07, 0.2, 0.07], [0, 1.11, -0.8], 0x424844);
    box(group, [1.04, 0.66, 0.09], [0, 1.46, -0.83], 0x343c39, 0.06);
    box(
      group,
      [0.91, 0.52, 0.018],
      [0, 1.46, -0.774],
      side ? 0xc6dbce : 0xe3ddd0,
      0.012,
    );
    for (let i = 0; i < 5; i++)
      box(
        group,
        [0.22 + (i % 3) * 0.14, 0.018, 0.009],
        [-0.19 + (i % 2) * 0.06, 1.6 - i * 0.067, -0.76],
        side ? 0x578875 : 0xa38562,
        0.002,
      );
    box(group, [0.65, 0.035, 0.23], [-0.15, 1.0, -0.33], 0xe7e7de, 0.02);
    for (let row = 0; row < 3; row++)
      for (let col = 0; col < 9; col++)
        box(
          group,
          [0.046, 0.01, 0.04],
          [-0.4 + col * 0.061, 1.024, -0.4 + row * 0.057],
          0xc3c7bc,
          0.004,
        );
    box(group, [0.12, 0.05, 0.19], [0.47, 1.02, -0.34], 0xe8e7df, 0.04);
    cylinder(
      group,
      0.095,
      0.18,
      [0.8, 1.06, -0.58],
      side ? 0xe97350 : 0x7c9484,
    );
    // Upholstered chair and crossed feet.
    cylinder(group, 0.035, 0.42, [0, 0.28, 0.55], 0x444b44);
    box(
      group,
      [0.68, 0.12, 0.6],
      [0, 0.56, 0.55],
      side ? 0xd77b57 : 0x637f70,
      0.09,
    );
    const chair = box(
      group,
      [0.68, 0.58, 0.13],
      [0, 0.88, 0.82],
      side ? 0xd77b57 : 0x637f70,
      0.09,
    );
    chair.rotation.x = -0.12;
    for (const r of [0, Math.PI / 2]) {
      const foot = box(group, [0.75, 0.04, 0.055], [0, 0.09, 0.55], 0x464c45);
      foot.rotation.y = r;
    }
    // Rug, plant and a low cabinet make each room feel inhabited.
    box(
      group,
      [1.5, 0.018, 1.4],
      [0, 0.064, 0.59],
      side ? 0xd8c4b6 : 0xd0d5c2,
      0.03,
    );
    cylinder(group, 0.22, 0.42, [1.48, 0.25, -1.2], 0xdfddd2, 0.28);
    for (let i = 0; i < 7; i++) {
      const leaf = box(
        group,
        [0.16, 0.65, 0.06],
        [
          1.48 + Math.sin(i * 2) * 0.13,
          0.7 + (i % 2) * 0.1,
          -1.2 + Math.cos(i * 2) * 0.12,
        ],
        i % 2 ? 0x637e5d : 0x819572,
        0.06,
      );
      leaf.rotation.z = Math.sin(i) * 0.5;
      leaf.rotation.y = i;
    }
    box(group, [0.7, 0.57, 0.63], [-1.43, 0.34, 0.72], 0xb69879, 0.045);
    box(group, [0.55, 0.018, 0.045], [-1.43, 0.36, 1.055], 0x735f4c);
    cylinder(group, 0.15, 0.045, [-1.43, 0.66, 0.72], 0x545d50);
    cylinder(group, 0.025, 0.4, [-1.43, 0.88, 0.72], 0x545d50);
    cylinder(group, 0.25, 0.24, [-1.43, 1.17, 0.72], 0xe8dfc3, 0.14);
    return group;
  }
  const left = room(0xd7a58a, false),
    right = room(0xb9cac8, true);
  const ground = box(world, [40, 0.08, 30], [0, -0.33, 0], 0xe9e5db, 0.01);
  const shadowMaterial = new THREE.ShadowMaterial({ opacity: 0.12 });
  ground.material = shadowMaterial;
  ground.castShadow = false;
  const wireMat = new THREE.MeshStandardMaterial({
    color: 0xe65c35,
    roughness: 0.45,
  });
  const curve = new THREE.CatmullRomCurve3([
    new THREE.Vector3(-2, 0.13, 0.8),
    new THREE.Vector3(-1, 0.13, 0.8),
    new THREE.Vector3(-0.7, 0.13, 1.5),
    new THREE.Vector3(0.7, 0.13, 1.5),
    new THREE.Vector3(1, 0.13, 0.8),
    new THREE.Vector3(2, 0.13, 0.8),
  ]);
  const wireGeo = new THREE.TubeGeometry(curve, 80, 0.035, 8, false);
  geometries.add(wireGeo);
  const wire = new THREE.Mesh(wireGeo, wireMat);
  world.add(wire);
  const beadGeo = new THREE.SphereGeometry(0.075, 12, 8);
  geometries.add(beadGeo);
  const beads = Array.from({ length: 4 }, () => {
    const b = new THREE.Mesh(beadGeo, mat(0xffce86));
    world.add(b);
    return b;
  });
  const bridge = box(world, [4, 0.1, 1.35], [0, -0.12, 0.9], 0xd8cfba, 0.03);
  let frame = 0,
    visible = true,
    px = 0,
    rotation = 0,
    disposed = false;
  const clock = new THREE.Timer();
  function render() {
    if (disposed || failed.value) return;
    clock.update();
    const time = clock.getElapsed();
    const p = props.progress;
    const join = THREE.MathUtils.smoothstep(p, 0.45, 1);
    left.position.x = -3.65 + join * 1.6;
    right.position.x = 3.65 - join * 1.6;
    left.rotation.y = -0.08 * (1 - join);
    right.rotation.y = 0.08 * (1 - join);
    right.userData.wall.scale.y = Math.max(0.001, 1 - join);
    right.userData.wall.position.y = 0.025 + 1.125 * (1 - join);
    world.rotation.y = rotation += (px * 0.07 - rotation) * 0.04;
    const reveal = THREE.MathUtils.smoothstep(p, 0.1, 0.5);
    wireGeo.setDrawRange(0, Math.floor((wireGeo.index.count * reveal) / 6) * 6);
    wire.scale.x = 1 - join * 0.8;
    bridge.scale.x = Math.max(0.001, 1 - join * 0.98);
    bridge.visible = p > 0.2;
    for (let i = 0; i < beads.length; i++) {
      beads[i].visible = p > 0.35;
      const point = curve.getPointAt(
        props.reduced ? i / 4 : (time * 0.19 + i / 4) % 1,
      );
      point.x *= wire.scale.x;
      beads[i].position.copy(point);
    }
    const narrow = host.value.clientWidth < 600;
    const distance = narrow ? 12 : 9;
    camera.position.set(distance - join * 1.3, 9 - join * 1.6, 13 + join);
    camera.lookAt(0, 1, 0);
    renderer.render(scene, camera);
    if (visible && !document.hidden && !props.reduced)
      frame = requestAnimationFrame(render);
  }
  function restart() {
    cancelAnimationFrame(frame);
    render();
  }
  function resize() {
    const w = host.value.clientWidth,
      h = host.value.clientHeight;
    if (!w || !h) return;
    const span = Math.max(w < 600 ? 7.7 : 7.3, (3.6 * w) / h);
    camera.left = -span;
    camera.right = span;
    camera.top = (span * h) / w;
    camera.bottom = (-span * h) / w;
    camera.updateProjectionMatrix();
    renderer.setSize(w, h);
    restart();
  }
  function pointer(e) {
    px = e.clientX / innerWidth - 0.5;
  }
  const observer = new ResizeObserver(resize);
  observer.observe(host.value);
  const io = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    restart();
  });
  io.observe(host.value);
  document.addEventListener("visibilitychange", restart);
  host.value.addEventListener("pointermove", pointer, { passive: true });
  const fallback = () => {
    failed.value = true;
    cancelAnimationFrame(frame);
  };
  renderer.domElement.addEventListener("webglcontextlost", fallback);
  const stop = watch(() => [props.progress, props.reduced], restart);
  cleanup = () => {
    disposed = true;
    stop();
    cancelAnimationFrame(frame);
    observer.disconnect();
    io.disconnect();
    document.removeEventListener("visibilitychange", restart);
    host.value?.removeEventListener("pointermove", pointer);
    renderer.domElement.removeEventListener("webglcontextlost", fallback);
    geometries.forEach((g) => g.dispose());
    materials.forEach((m) => m.dispose());
    wireMat.dispose();
    shadowMaterial.dispose();
    clock.dispose();
    renderer.dispose();
    renderer.domElement.remove();
  };
});
onBeforeUnmount(() => cleanup());
</script>
<template>
  <div
    ref="host"
    class="room-render"
    role="img"
    :aria-label="
      language === 'zh-CN'
        ? '两个微缩房间随滚动连接在一起'
        : 'Two miniature rooms connect as you scroll'
    "
  >
    <div v-if="failed" class="room-fallback">
      <span>⌨</span><b>↔</b><span>⌨</span>
      <p>
        {{
          language === "zh-CN"
            ? "两台设备，一个房间号。"
            : "Two devices. One room code."
        }}
      </p>
    </div>
  </div>
</template>
