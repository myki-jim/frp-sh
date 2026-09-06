import * as T from "three";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";
import {
  CSS3DRenderer,
  CSS3DObject,
} from "three/addons/renderers/CSS3DRenderer.js";
import { chapterAt, chapters, stops, commands } from "./room-story.js";

// Everything visible belongs to the world: printed surfaces, equipment, or screens.
export function createRoomWorld(host, state, action, report) {
  const scene = new T.Scene();
  const camera = new T.PerspectiveCamera(40, 1, 0.1, 180);
  const renderer = new T.WebGLRenderer({
    antialias: true,
    powerPreference: "low-power",
  });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 1.5));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = T.PCFSoftShadowMap;
  renderer.shadowMap.autoUpdate = false;
  renderer.shadowMap.needsUpdate = true;
  renderer.toneMapping = T.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.1;
  host.appendChild(renderer.domElement);
  renderer.domElement.setAttribute("aria-label", "Interactive miniature rooms");
  const css = new CSS3DRenderer();
  css.domElement.className = "world-surfaces";
  host.appendChild(css.domElement);
  const geo = new Map(),
    materials = new Map(),
    textures = [],
    custom = [],
    surfaces = [],
    occluders = [];
  const vector = (a) => new T.Vector3(...a);
  const mix = T.MathUtils.lerp,
    smooth = T.MathUtils.smoothstep;
  function material(color, metal = 0) {
    const key = `${color}/${metal}`;
    if (!materials.has(key))
      materials.set(
        key,
        new T.MeshStandardMaterial({
          color,
          roughness: metal ? 0.4 : 0.8,
          metalness: metal,
        }),
      );
    return materials.get(key);
  }
  function box(parent, size, pos, color, radius = 0.025) {
    const key = `${size}/${radius}`;
    if (!geo.has(key))
      geo.set(
        key,
        new RoundedBoxGeometry(
          ...size,
          1,
          Math.min(radius, Math.min(...size) / 3),
        ),
      );
    const mesh = new T.Mesh(geo.get(key), material(color));
    mesh.position.set(...pos);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    parent.add(mesh);
    if (size[0] * size[1] * size[2] > 0.09) occluders.push(mesh);
    return mesh;
  }
  function cyl(parent, r, h, pos, color, top = r) {
    const key = `c${r}/${h}/${top}`;
    if (!geo.has(key)) geo.set(key, new T.CylinderGeometry(top, r, h, 16));
    const m = new T.Mesh(geo.get(key), material(color));
    m.position.set(...pos);
    m.castShadow = true;
    m.receiveShadow = true;
    parent.add(m);
    return m;
  }
  function label(
    parent,
    w,
    h,
    pos,
    { color = "#23352f", bg = "#e9e0cc", size = 42, glow = false } = {},
  ) {
    const canvas = document.createElement("canvas");
    canvas.width = 1024;
    canvas.height = Math.round((1024 * h) / w);
    const texture = new T.CanvasTexture(canvas);
    texture.colorSpace = T.SRGBColorSpace;
    texture.anisotropy = 4;
    textures.push(texture);
    const mat = glow
      ? new T.MeshBasicMaterial({ map: texture, toneMapped: false })
      : new T.MeshStandardMaterial({ map: texture, roughness: 0.9 });
    custom.push(mat);
    const key = `p${w}/${h}`;
    if (!geo.has(key)) geo.set(key, new T.PlaneGeometry(w, h));
    const mesh = new T.Mesh(geo.get(key), mat);
    mesh.position.set(...pos);
    parent.add(mesh);
    let last = "";
    const draw = (text, accent = false) => {
      const token = text + accent;
      if (token === last) return;
      last = token;
      const ctx = canvas.getContext("2d");
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.fillStyle = accent ? "#b84f2f" : color;
      ctx.font = `500 ${size}px ui-monospace, "Microsoft YaHei", monospace`;
      ctx.textBaseline = "middle";
      const lines = text.split("\n");
      const lineHeight = size * 1.45;
      lines.forEach((line, i) =>
        ctx.fillText(
          line,
          45,
          canvas.height / 2 + (i - (lines.length - 1) / 2) * lineHeight,
          930,
        ),
      );
      texture.needsUpdate = true;
    };
    return { mesh, draw, canvas, texture };
  }
  function surface(parent, width, height, pos, element) {
    const item = new CSS3DObject(element);
    element.style.width = "1000px";
    element.style.height = `${(1000 * height) / width}px`;
    item.scale.setScalar(width / 1000);
    item.position.set(...pos);
    parent.add(item);
    surfaces.push(item);
    return item;
  }
  function plant(parent, x, z, scale = 1) {
    const g = new T.Group();
    g.position.set(x, 0.06, z);
    g.scale.setScalar(scale);
    parent.add(g);
    cyl(g, 0.19, 0.35, [0, 0.175, 0], 0xc7b69c, 0.24);
    for (let i = 0; i < 7; i++) {
      const leaf = box(
        g,
        [0.17, 0.64, 0.055],
        [
          Math.sin(i * 2.4) * 0.14,
          0.57 + (i % 3) * 0.08,
          Math.cos(i * 2.4) * 0.14,
        ],
        i % 2 ? 0x597451 : 0x7b8c60,
        0.05,
      );
      leaf.rotation.set(Math.cos(i) * 0.4, i, Math.sin(i) * 0.4);
    }
  }
  function books(parent, pos, count = 6) {
    for (let i = 0; i < count; i++)
      box(
        parent,
        [0.1, 0.24 + (i % 3) * 0.07, 0.23],
        [pos[0] + i * 0.115, pos[1], pos[2]],
        [0xb96d4e, 0xd9cdb3, 0x57736d, 0x525969][i % 4],
      );
  }
  const hemi = new T.HemisphereLight(0xf1f2ed, 0x77766b, 2.5);
  scene.add(hemi);
  const sun = new T.DirectionalLight(0xffead0, 3.1);
  sun.position.set(-8, 15, -9);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  Object.assign(sun.shadow.camera, {
    left: -20,
    right: 20,
    top: 20,
    bottom: -20,
    far: 70,
  });
  sun.shadow.normalBias = 0.045;
  scene.add(sun);
  const fill = new T.DirectionalLight(0xb1c8eb, 0.7);
  fill.position.set(5, 9, 12);
  scene.add(fill);
  const floor = box(scene, [500, 0.1, 500], [0, -0.42, 0], 0xd6d2c7);
  floor.castShadow = false;
  const groundMat = new T.ShadowMaterial({ color: 0x080d14, opacity: 0.18 });
  custom.push(groundMat);
  floor.material = groundMat;
  const names = [
    ["THE STUDY", "木作书房"],
    ["THE LOFT", "开发者阁楼"],
    ["THE STUDIO", "工业工作室"],
    ["AFTER HOURS", "游戏卧室"],
    ["THE GREENHOUSE", "植物阳光房"],
    ["THE WORKSHOP", "电子工坊"],
  ];
  const palettes = [
    [0xe8d6ba, 0xc9a275, 0x6c8074],
    [0xd3cbbd, 0x7c5740, 0x495963],
    [0xa8aca7, 0x5a6461, 0xa16146],
    [0x9aa5a7, 0x4c555e, 0x9c6959],
    [0xc6d0b7, 0xd0b687, 0x8b966d],
    [0xd8c8ac, 0x947151, 0x6e7b61],
  ];
  const starts = [
    [0, 5],
    [-7, 5],
    [7, 5],
    [-7, -4],
    [0, -4],
    [7, -4],
  ];
  const ends = [
    [0, 2.2],
    [-4.5, 2.2],
    [4.5, 2.2],
    [-4.5, -2.2],
    [0, -2.2],
    [4.5, -2.2],
  ];
  const rooms = [];
  for (let n = 0; n < 6; n++) {
    const g = new T.Group();
    scene.add(g);
    const [wall, wood, seat] = palettes[n];
    box(g, [4.45, 0.22, 4.3], [0, -0.12, 0], wood, 0.07);
    box(
      g,
      [4.4, 0.06, 4.24],
      [0, 0.02, 0],
      n === 2 ? 0x92978f : n === 4 ? 0xd1ccbb : n === 3 ? 0x8f8b86 : 0xcfc2a9,
    );
    for (let i = 0; i < 11; i++)
      box(
        g,
        [0.008, 0.006, 4.22],
        [-2 + i * 0.4, 0.055, 0],
        n === 2 ? 0x858d85 : 0xb1a58f,
        0.001,
      );
    if (n === 2 || n === 4)
      for (let i = 0; i < 6; i++)
        box(g, [4.4, 0.006, 0.012], [0, 0.055, -2 + i * 0.8], 0xaaa995, 0.001);
    // Real opening, sill, mullions and glass: daylight can pass through the window.
    const sideWall = box(g, [0.12, 2.65, 4.25], [-2.2, 1.36, 0], wall);
    const back = new T.Group();
    g.add(back);
    const winW = n === 4 ? 3.65 : n === 2 ? 2.3 : 1.7;
    box(
      back,
      [(4.4 - winW) / 2, 2.65, 0.12],
      [-(winW / 2 + (4.4 - winW) / 4), 1.36, -2.08],
      wall,
    );
    box(
      back,
      [(4.4 - winW) / 2, 2.65, 0.12],
      [winW / 2 + (4.4 - winW) / 4, 1.36, -2.08],
      wall,
    );
    box(back, [winW, 0.68, 0.12], [0, 0.38, -2.08], wall);
    box(back, [winW, 0.36, 0.12], [0, 2.5, -2.08], wall);
    const glassMat = new T.MeshPhysicalMaterial({
      color: 0xc2d8db,
      transparent: true,
      opacity: 0.17,
      roughness: 0.2,
      depthWrite: false,
    });
    custom.push(glassMat);
    const glass = box(back, [winW, 1.5, 0.025], [0, 1.52, -2.075], 0xffffff);
    glass.material = glassMat;
    glass.castShadow = false;
    for (const x of [-winW / 2, 0, winW / 2])
      box(
        back,
        [0.06, 1.65, 0.16],
        [x, 1.52, -2.05],
        n === 2 ? 0x414b4b : 0xe9dfca,
      );
    box(back, [winW + 0.15, 0.08, 0.36], [0, 0.71, -1.99], wood);
    const desk = new T.Group();
    g.add(desk);
    box(desk, [2.65, 0.12, 1.02], [0, 1.02, -0.62], wood, 0.06);
    for (const x of [-1.12, 1.12])
      for (const z of [-0.98, -0.25])
        box(desk, [0.065, 1, 0.065], [x, 0.51, z], 0x414b45);
    box(desk, [0.44, 0.06, 0.35], [0, 1.12, -0.84], 0x3e4b45);
    box(desk, [0.085, 0.22, 0.08], [0, 1.25, -0.91], 0x3e4b45);
    const monitorFrame = box(
      desk,
      [n === 5 ? 1.3 : 1.85, 1.02, n === 5 ? 0.48 : 0.14],
      [0, 1.79, -0.91],
      n === 5 ? 0xc5c0a7 : 0x303a34,
      0.06,
    );
    const display = label(
      desk,
      n === 5 ? 1.12 : 1.7,
      0.88,
      [0, 1.79, n === 5 ? -0.661 : -0.827],
      {
        bg: n === 5 ? "#17271f" : "#182c27",
        color: "#c9dbc6",
        size: 67,
        glow: true,
      },
    );
    box(desk, [0.9, 0.05, 0.31], [-0.08, 1.11, -0.27], 0xd9d4c6);
    for (let row = 0; row < 3; row++)
      for (let col = 0; col < 10; col++)
        box(
          desk,
          [0.062, 0.014, 0.065],
          [-0.45 + col * 0.08, 1.144, -0.37 + row * 0.083],
          0xb8bcae,
          0.008,
        );
    box(desk, [0.13, 0.06, 0.22], [0.65, 1.12, -0.26], 0xd8d6c8, 0.04);
    cyl(desk, 0.1, 0.22, [1.02, 1.18, -0.65], seat);
    const chair = new T.Group();
    g.add(chair);
    chair.position.set(0, 0, 0.75);
    cyl(chair, 0.04, 0.49, [0, 0.3, 0], 0x3d4740);
    box(chair, [0.74, 0.13, 0.67], [0, 0.58, 0], seat, 0.06);
    box(chair, [0.74, 0.65, 0.14], [0, 0.94, 0.29], seat, 0.06);
    box(chair, [0.85, 0.05, 0.07], [0, 0.09, 0], 0x414b45);
    box(chair, [0.07, 0.05, 0.85], [0, 0.09, 0], 0x414b45);
    box(g, [1.65, 0.015, 1.8], [0, 0.065, 0.73], n === 3 ? 0x776d6e : 0xb6b8a2);
    // Each room has a different silhouette and lived-in objects.
    if (n === 0) {
      box(g, [0.65, 1.18, 1.2], [-1.65, 0.63, -0.3], wood);
      for (let i = 0; i < 3; i++) books(g, [-1.92, 0.38 + i * 0.35, -0.12], 5);
      plant(g, 1.7, -1.6, 0.9);
      for (const x of [-1, 1])
        for (let j = 0; j < 5; j++)
          box(
            back,
            [0.08, 1.55, 0.09],
            [x * (winW / 2 + 0.1) + j * 0.035, 1.55, -1.94],
            0xddd5c3,
          );
    } else if (n === 1) {
      for (const z of [-1.9, 1.85]) {
        for (const sign of [-1, 1]) {
          const beam = box(g, [2.5, 0.14, 0.16], [sign * 1.1, 2.98, z], wood);
          beam.rotation.z = -sign * 0.4;
        }
      }
      box(g, [0.14, 2.6, 0.14], [2.05, 1.3, -1.8], wood);
      box(g, [0.16, 0.16, 4.3], [0, 3.45, 0], wood);
      const roof = box(g, [2.48, 0.07, 2.2], [-1.1, 3.0, -0.96], 0xc0b59d);
      roof.rotation.z = 0.4;
      for (let i = 0; i < 4; i++)
        box(g, [0.6, 0.04, 0.05], [-1.25, 2.24 + i * 0.12, -1.97], 0x6c7a7b);
      box(g, [0.92, 0.65, 0.1], [1.24, 1.67, -0.83], 0x354246);
      const extra = label(g, 0.81, 0.54, [1.24, 1.67, -0.77], {
        bg: "#1b292e",
        color: "#b8cecf",
        size: 65,
        glow: true,
      });
      extra.draw("git status\nworking tree clean");
      box(g, [0.63, 0.1, 0.8], [1.65, 0.24, 0.9], 0x626a63);
      plant(g, -1.65, 1.5, 0.7);
    } else if (n === 2) {
      for (const y of [0.35, 1.15, 1.95])
        box(g, [0.55, 0.06, 2.2], [1.83, y, -0.2], 0x535d58);
      for (const z of [-1.2, 0.8])
        box(g, [0.06, 2.4, 0.06], [1.83, 1.2, z], 0x424c46);
      for (let i = 0; i < 5; i++)
        box(
          g,
          [0.37, 0.25, 0.31],
          [1.82, 0.52, -0.95 + i * 0.4],
          i % 2 ? 0xbb8766 : 0x878f7a,
        );
      box(g, [0.75, 0.16, 0.55], [-1.58, 0.15, 1.38], 0x7f887c);
      cyl(g, 0.22, 0.1, [-1.55, 0.3, 1.4], 0x3d4740);
    } else if (n === 3) {
      box(g, [0.9, 0.32, 2.05], [1.6, 0.23, 0.4], wood, 0.08);
      box(g, [0.92, 0.17, 2], [1.6, 0.47, 0.4], 0xc0b9b0, 0.08);
      box(g, [0.8, 0.15, 0.52], [1.6, 0.61, -0.2], 0xe0d4bd, 0.08);
      box(g, [0.95, 0.07, 1.1], [1.6, 0.59, 0.8], 0x7d8985);
      for (const x of [-1, 1])
        box(back, [0.4, 1.65, 0.08], [x * 0.87, 1.55, -1.92], 0x48585d);
      box(desk, [0.34, 0.075, 0.18], [-0.85, 1.14, -0.23], 0x424a43, 0.04);
    } else if (n === 4) {
      sideWall.material = glassMat;
      for (const z of [-2, -0.6, 0.7, 2])
        box(g, [0.08, 2.8, 0.08], [-2.2, 1.4, z], 0x6c816c);
      box(g, [0.08, 0.08, 4.3], [-2.2, 2.8, 0], 0x6c816c);
      for (const z of [-2, 0, 2])
        box(g, [4.4, 0.08, 0.08], [0, 2.8, z], 0x6c816c);
      for (const [x, z, s] of [
        [-1.65, -1.5, 1.4],
        [1.7, -1.6, 1.7],
        [1.65, 1.4, 1],
        [-1.65, 1.3, 0.7],
      ])
        plant(g, x, z, s);
      for (let i = 0; i < 5; i++)
        box(
          chair,
          [0.07, 0.67, 0.07],
          [-0.28 + i * 0.14, 0.94, 0.31],
          0xb99b70,
        );
      box(g, [0.65, 0.06, 0.65], [1.6, 0.7, 0.8], wood);
      cyl(g, 0.13, 0.2, [1.6, 0.83, 0.8], 0x698678);
    } else {
      for (let i = 0; i < 4; i++)
        for (let j = 0; j < 3; j++)
          box(
            g,
            [0.43, 0.28, 0.47],
            [-1.75 + j * 0.01, 0.25 + i * 0.31, -1.1 + j * 0.55],
            i % 2 ? 0x9f9a78 : 0xb0a785,
          );
      box(desk, [0.48, 0.16, 0.34], [0.9, 1.18, -0.76], 0x747e63);
      cyl(desk, 0.04, 0.3, [0.98, 1.4, -0.76], 0x454d42);
      books(g, [0.95, 0.23, 1.5], 6);
      for (let x = 0; x < 6; x++)
        for (let y = 0; y < 4; y++)
          box(
            back,
            [0.23, 0.22, 0.05],
            [-0.7 + x * 0.27, 1 + y * 0.29, -1.98],
            (x + y) % 2 ? 0xa7bab2 : 0xb7c8bb,
          );
      box(g, [0.75, 0.55, 0.48], [1.6, 0.33, 1.45], 0x7b806a);
    }
    const lamp = new T.Group();
    g.add(lamp);
    lamp.position.set(-1.65, 0, 0.7);
    cyl(lamp, 0.22, 0.07, [0, 0.14, 0], 0x465449);
    cyl(lamp, 0.026, 1.45, [0, 0.88, 0], 0x465449);
    const shade = cyl(
      lamp,
      0.34,
      0.32,
      [0, 1.66, 0],
      n === 3 ? 0xb78362 : 0xe6d7b5,
      0.2,
    );
    const lampMat = material(n === 3 ? 0xb78362 : 0xe6d7b5).clone();
    lampMat.emissive.set(0xffc080);
    shade.material = lampMat;
    custom.push(lampMat);
    const light = new T.PointLight(n === 5 ? 0xe4edba : 0xffc18a, 0, 5.5, 2);
    light.position.set(-1.65, 1.45, 0.7);
    g.add(light);
    const plaque = box(g, [2.75, 0.56, 0.09], [0, 0.53, 2.17], 0x495b50);
    const name = label(g, 2.62, 0.44, [0, 0.53, 2.222], {
      bg: "#495b50",
      color: "#ede2c9",
      size: 65,
    });
    rooms.push({
      g,
      sideWall,
      back,
      display,
      name,
      chair,
      light,
      lampMat,
      desk,
      monitorFrame,
    });
  }
  // A shared entrance sign remains attached to an actual frame.
  const arch = new T.Group();
  scene.add(arch);
  for (const x of [-2.65, 2.65])
    box(arch, [0.13, 3.3, 0.13], [x, 1.65, 0], 0x576a59);
  box(arch, [5.5, 0.88, 0.13], [0, 3.17, 0], 0xded4bd);
  const archText = label(arch, 5.22, 0.72, [0, 3.17, 0.071], { size: 55 });
  arch.position.set(0, 0, 8.4);
  const manual = box(
    rooms[0].g,
    [0.65, 0.055, 0.38],
    [0.6, 1.11, -0.25],
    0xba7558,
  );
  manual.rotation.y = -0.12;
  const title = label(rooms[0].g, 3.7, 0.88, [0, 2.96, -1.98], {
    bg: "#d7c9af",
    size: 61,
  });
  box(rooms[0].g, [3.82, 1, 0.08], [0, 2.96, -2.035], 0xa48c65);
  const helper = label(rooms[0].g, 1.02, 0.8, [1.6, 1.57, -0.23], {
    bg: "#26372b",
    color: "#c1ceae",
    size: 50,
    glow: true,
  });
  box(rooms[0].g, [1.13, 0.9, 0.12], [1.6, 1.57, -0.3], 0x6c775b);
  box(rooms[0].g, [0.5, 1.07, 0.7], [1.6, 0.54, -0.36], 0x827d61);
  box(rooms[0].g, [0.6, 0.08, 0.75], [1.6, 1.1, -0.36], 0x827d61);
  const authorizationCard = box(
    rooms[0].g,
    [0.34, 0.018, 0.22],
    [1.6, 0.85, 0.03],
    0xbba77f,
  );
  box(rooms[0].g, [0.4, 0.04, 0.015], [1.6, 0.85, 0.002], 0x35483b);
  const rustPlate = label(rooms[0].g, 0.45, 0.2, [1.6, 0.55, 0.001], {
    bg: "#827d61",
    color: "#e7d8b7",
    size: 170,
  });
  rustPlate.draw("RUST");
  const routeBoard = new T.Group();
  scene.add(routeBoard);
  box(routeBoard, [3.6, 0.84, 0.13], [0, 1.3, 0], 0xc9bfa7);
  for (const x of [-1.5, 1.5])
    box(routeBoard, [0.08, 1.2, 0.08], [x, 0.6, 0], 0x65705c);
  const routeText = label(routeBoard, 3.4, 0.7, [0, 1.3, 0.08], { size: 46 });
  routeBoard.position.set(4, 0, 5.3);
  const routeMaterial = new T.MeshStandardMaterial({
    color: 0xcc653d,
    emissive: 0xc96132,
    emissiveIntensity: 0.28,
    roughness: 0.5,
  });
  custom.push(routeMaterial);
  const links = [];
  // Curves are updated in-place when rooms merge; no new geometry per frame.
  for (let i = 1; i < 6; i++) {
    const points = Array.from({ length: 40 }, () => new T.Vector3());
    const geometry = new T.BufferGeometry().setFromPoints(points);
    geo.set(`link${i}`, geometry);
    const line = new T.Line(
      geometry,
      new T.LineBasicMaterial({ color: 0xc77d49 }),
    );
    custom.push(line.material);
    scene.add(line);
    const pulse = cyl(scene, 0.075, 0.08, [0, 0.14, 0], 0xf4d3a2);
    pulse.material = routeMaterial;
    links.push({ line, pulse, index: i });
  }
  const relay = box(scene, [0.55, 0.3, 0.55], [6.8, 0.16, 4.7], 0x72806c);
  relay.visible = false;
  const relayLabel = label(scene, 1.9, 0.4, [6.8, 0.65, 4.7], { size: 80 });
  relayLabel.draw("TURN / TCP");
  // The install screen is a selectable DOM surface registered in the same 3D world.
  const terminal = document.createElement("div");
  terminal.className = "world-terminal";
  terminal.innerHTML =
    '<div class="terminal-brand">frp.sh <span>v0.4.0</span></div><h1></h1><p class="terminal-platform"></p><pre tabindex="0"></pre><p class="terminal-status" role="status"></p><p class="terminal-note"></p>';
  const screen = surface(rooms[0].g, 1.7, 0.88, [0, 1.79, -0.824], terminal);
  // Controls have real modeled keycaps. HTML only supplies invisible semantic hit surfaces.
  const controller = new T.Group();
  scene.add(controller);
  box(controller, [3.66, 0.16, 0.55], [0, 0, 0], 0xbba885, 0.05);
  box(controller, [0.09, 1, 0.09], [-1.52, -0.5, 0], 0x536050);
  box(controller, [0.09, 1, 0.09], [1.52, -0.5, 0], 0x536050);
  const controls = [];
  let dayDial;
  const definitions = [
    ["previous", "←"],
    ["next", "→"],
    ["language", "EN / 中文"],
    ["day", "DAY"],
    ["install", "INSTALL"],
  ];
  for (let i = 0; i < definitions.length; i++) {
    const [id, caption] = definitions[i],
      x = -1.43 + i * 0.715;
    const key = box(
      controller,
      [0.66, 0.36, 0.12],
      [x, 0.18, 0.23],
      i === 4 ? 0xaa6245 : 0xd8cbae,
      0.025,
    );
    const printed = label(controller, 0.61, 0.28, [x, 0.18, 0.296], {
      bg: i === 4 ? "#aa6245" : "#d8cbae",
      color: i === 4 ? "#fff1d5" : "#364237",
      size: 220,
    });
    printed.draw(caption);
    const el = document.createElement("button");
    el.className = "physical-hit";
    el.type = "button";
    el.addEventListener("click", () => action(id));
    surface(
      controller,
      0.66,
      host.clientWidth < 600 ? 0.72 : 0.36,
      [x, 0.18, 0.299],
      el,
    );
    controls.push({ id, el, printed, key });
    el.addEventListener("pointerdown", () => {
      key.position.z = 0.21;
      refresh();
    });
    for (const event of ["pointerup", "pointerleave", "blur"])
      el.addEventListener(event, () => {
        key.position.z = 0.23;
        refresh();
      });
    if (id === "day") {
      dayDial = new T.Group();
      controller.add(dayDial);
      dayDial.position.set(x + 0.19, 0.18, 0.39);
      const dial = cyl(dayDial, 0.1, 0.08, [0, 0, 0], 0xa5ab89);
      dial.rotation.x = Math.PI / 2;
      box(dayDial, [0.015, 0.075, 0.012], [0, 0.02, 0.048], 0x4b5946, 0.002);
    }
  }
  // Buttons attached to the monitor plinth at the final stop.
  const installKeys = new T.Group();
  rooms[0].g.add(installKeys);
  const keyDefs = [
    ["platform", "SYSTEM"],
    ["copy", "COPY"],
    ["guide", "GUIDE"],
    ["source", "SOURCE"],
  ];
  const finalButtons = [];
  keyDefs.forEach(([id, caption], i) => {
    const x = -0.69 + i * 0.46;
    box(
      installKeys,
      [0.42, 0.24, 0.11],
      [x, 1.24, -0.78],
      id === "copy" ? 0xaf674c : 0x687560,
      0.025,
    );
    const printed = label(installKeys, 0.39, 0.21, [x, 1.24, -0.718], {
      bg: id === "copy" ? "#af674c" : "#687560",
      color: "#fff0d4",
      size: 250,
    });
    printed.draw(caption);
    const el = document.createElement("button");
    el.className = "physical-hit";
    el.type = "button";
    el.addEventListener("click", () => action(id));
    surface(installKeys, 0.42, 0.24, [x, 1.24, -0.711], el);
    finalButtons.push({ id, el, printed });
  });
  const dayColor = new T.Color(0xdad8cc),
    nightColor = new T.Color(0x171f2b),
    warmColor = new T.Color(0xd5b391);
  let width = 1,
    height = 1,
    frame = 0,
    disposed = false,
    night =
      state.mode === "night" || (state.mode === "auto" && state.systemDark)
        ? 1
        : 0,
    previousTime = 0,
    lastLabels = "",
    lastTerminal = "",
    stage = -1,
    lastOcclusion = 0,
    shadowProgress = -1,
    shadowNight = -1,
    lastActivity = -1;
  const ray = new T.Raycaster();
  const getFocus = (p) => {
    const hero = rooms[0].g.position,
      screenY = width < 600 ? 2 : 1.79;
    const frames = [
      { eye: [5.7, 5.1, 13.4], at: [0.4, 1.4, 5.5] },
      { eye: [1.8, 3.5, 9.6], at: [0, 1.7, 4.3] },
      { eye: [13, 14, 23], at: [0, 1, 1.5] },
      { eye: [8.5, 11, 18], at: [0, 1, 1] },
      { eye: [-7.8, 6.2, 11.7], at: [-3.5, 1, 1] },
      { eye: [8.5, 5, 12.5], at: [4.8, 0.6, 3] },
      { eye: [3.5, 3.5, 7.8], at: [0.8, 1.4, 2] },
      {
        eye: [
          hero.x,
          screenY,
          hero.z -
            0.824 +
            Math.max(2.5, 1.05 / (Math.tan(Math.PI / 9) * camera.aspect)),
        ],
        at: [hero.x, screenY, hero.z - 0.824],
      },
    ];
    let k = 0;
    while (k < stops.length - 2 && p > stops[k + 1]) k++;
    const f = smooth(p, stops[k], stops[k + 1]);
    const eye = vector(frames[k].eye).lerp(vector(frames[k + 1].eye), f);
    const at = vector(frames[k].at).lerp(vector(frames[k + 1].at), f);
    if (camera.aspect < 0.8 && k < 6) {
      eye
        .sub(at)
        .multiplyScalar(k === 2 || k === 3 ? 1.8 : 1.35)
        .add(at);
      if (k < 2) {
        const close = smooth(p, 0, 0.13) * (1 - smooth(p, 0.13, 0.3));
        at.set(0, 1.5, 4.8);
        eye.set(mix(2, 0.5, close), mix(7, 4, close), mix(21, 16, close));
      }
    }
    return { eye, at };
  };
  function paintText(p) {
    const zh = state.language === "zh-CN",
      s = chapterAt(p),
      members = Math.min(6, Math.max(1, Math.floor((p - 0.17) * 35) + 1)),
      key = `${zh}/${s}/${state.mode}/${members}`;
    if (key !== lastLabels) {
      lastLabels = key;
      rooms.forEach((r, i) => {
        r.name.draw(
          `${String(i + 1).padStart(2, "0")}  ${names[i][zh ? 1 : 0]}`,
        );
        r.display.draw(
          i === 0
            ? s < 1
              ? zh
                ? "frp.sh\n滚动，开始连接 ↓"
                : "frp.sh\nSCROLL TO CONNECT ↓"
              : zh
                ? `orbit-2048\n${members} 位成员\n房间演示`
                : `orbit-2048\n${members} members\nDEMO ROOM`
            : i >= members
              ? zh
                ? "frp.sh\n等待邀请"
                : "frp.sh\nwaiting for invitation"
              : zh
                ? `${names[i][1]}\norbit-2048\n已连接`
                : `${names[i][0]}\norbit-2048\nconnected`,
        );
      });
      title.draw(
        zh ? "你的桌面。\n大家的房间。" : "Your desk.\nA room for everyone.",
      );
      archText.draw(chapters[Math.min(Math.max(s, 2), 4)][zh ? 1 : 0]);
      routeText.draw(
        zh
          ? "能够直连，就直达。\n需要时，中继接力。"
          : "Direct when possible.\nA relay when needed.",
      );
      helper.draw(
        zh
          ? "安装时授权\n日常普通账户\n独立日志 · Rust"
          : "INSTALL: AUTHORIZE\nCONNECT: YOUR ACCOUNT\nSEPARATE LOGS / RUST",
      );
      const labels = {
        previous: zh ? "上一幕" : "Previous scene",
        next: zh ? "下一幕" : "Next scene",
        language: zh ? "Switch to English" : "切换到中文",
        day: zh ? "切换昼夜模式" : "Change day or night",
        install: zh ? "跳到安装" : "Jump to installation",
      };
      controls.forEach(({ id, el, printed }) => {
        el.setAttribute("aria-label", labels[id]);
        if (id === "day")
          printed.draw(
            state.mode === "auto"
              ? "AUTO"
              : state.mode === "night"
                ? zh
                  ? "夜晚"
                  : "NIGHT"
                : zh
                  ? "白天"
                  : "DAY",
          );
        if (id === "install") printed.draw(zh ? "安装" : "INSTALL");
      });
      finalButtons.forEach(({ id, el, printed }) => {
        const labels = {
          platform: zh ? "切换操作系统" : "Change operating system",
          copy: zh ? "复制安装命令" : "Copy installation command",
          guide: zh ? "安装指南" : "Installation guide",
          source: zh ? "源代码" : "Source code",
        };
        el.setAttribute("aria-label", labels[id]);
        printed.draw(
          {
            platform: zh ? "系统" : "SYSTEM",
            copy: zh ? "复制" : "COPY",
            guide: zh ? "指南" : "GUIDE",
            source: zh ? "源码" : "SOURCE",
          }[id],
        );
      });
    }
    const tkey = `${zh}/${state.platform}/${state.message}`;
    if (tkey !== lastTerminal) {
      lastTerminal = tkey;
      terminal.querySelector("h1").textContent = zh
        ? "你的下一个房间，从这里开始。"
        : "Your next room starts here.";
      terminal.querySelector(".terminal-platform").textContent =
        "> " + (state.platform === "windows" ? "Windows" : "macOS / Linux");
      terminal.querySelector("pre").textContent = commands[state.platform];
      terminal.querySelector(".terminal-status").textContent =
        state.message ||
        (zh
          ? "按下显示器底座的「复制」键。"
          : "Press COPY on the monitor below.");
      terminal.querySelector(".terminal-note").textContent = zh
        ? "安装辅助服务后，运行 frp-sh 配置你的信令服务器。"
        : "Installs a privileged helper. Run frp-sh to configure your signaling server.";
    }
    if (s !== stage) {
      stage = s;
      report(s);
    }
  }
  function render(ms = performance.now()) {
    if (disposed) return;
    const dt = Math.min(0.05, (ms - previousTime) / 1000 || 0.016);
    previousTime = ms;
    const p = state.progress,
      join = smooth(p, 0.34, 0.48),
      zh = state.language === "zh-CN";
    const targetNight =
      state.mode === "night" || (state.mode === "auto" && state.systemDark)
        ? 1
        : 0;
    night = state.reduced
      ? targetNight
      : mix(night, targetNight, 1 - Math.exp(-dt * 1.9));
    const sunset = Math.sin(night * Math.PI) * 0.35;
    if (dayDial) dayDial.rotation.z = night * -1.7;
    scene.background = dayColor
      .clone()
      .lerp(nightColor, night)
      .lerp(warmColor, sunset);
    groundMat.opacity = mix(0.18, 0.35, night);
    hemi.intensity = mix(2.2, 0.48, night);
    sun.intensity = mix(2.7, 0.16, night);
    fill.intensity = mix(0.6, 0.75, night);
    sun.color.setRGB(1, mix(0.91, 0.71, sunset), mix(0.77, 0.48, sunset));
    sun.position.set(mix(-8, 3, night), mix(15, 4, night), -9);
    authorizationCard.position.z = mix(0.2, -0.2, smooth(p, 0.81, 0.86));
    rooms.forEach((r, i) => {
      r.g.position.set(
        mix(starts[i][0], ends[i][0], join),
        0,
        mix(starts[i][1], ends[i][1], join),
      );
      r.sideWall.scale.y = Math.max(0.025, 1 - join * 0.98);
      r.sideWall.position.y = 1.36 * (1 - join * 0.98);
      r.light.intensity = night * (i === 4 ? 8 : 5);
      r.lampMat.emissiveIntensity = night * 0.55;
      r.chair.position.x = i === 0 ? smooth(p, 0.84, 0.98) * 1.2 : 0;
    });
    arch.position.z = mix(8.4, 5.5, join);
    arch.position.y = -smooth(p, .48, .55) * 4.2;
    arch.visible = p > 0.2 && p < 0.55;
    routeBoard.visible = p > 0.68 && p < 0.83;
    relay.visible = routeBoard.visible;
    relayLabel.mesh.visible = relay.visible;
    links.forEach(({ line, pulse, index }) => {
      const show = smooth(p, 0.16 + index * 0.019, 0.29 + index * 0.019);
      line.visible = show > 0;
      pulse.visible = show > 0.2;
      const a = rooms[0].g.position,
        b = rooms[index].g.position,
        attrs = line.geometry.attributes.position;
      const detour = index === 2 ? smooth(p, 0.7, 0.76) : 0;
      function route(f) {
        const half = f < 0.5 ? f * 2 : (f - 0.5) * 2;
        const rx = f < 0.5 ? mix(a.x, 6.8, half) : mix(6.8, b.x, half);
        const rz = f < 0.5 ? mix(a.z, 4.7, half) : mix(4.7, b.z, half);
        return [
          mix(mix(a.x, b.x, f), rx, detour),
          mix(mix(a.z, b.z, f) + Math.sin(f * Math.PI) * 1.1, rz, detour),
        ];
      }
      for (let j = 0; j < 40; j++) {
        const [x, z] = route(j / 39);
        attrs.setXYZ(j, x, 0.12, z);
      }
      line.material.color.setHex(detour > 0.5 ? 0xcf7045 : 0x8c9e63);
      attrs.needsUpdate = true;
      line.geometry.setDrawRange(0, Math.floor(40 * show));
      line.geometry.computeBoundingSphere();
      const f = state.reduced ? 0.5 : (ms * 0.00022 + index * 0.18) % 1;
      const [x, z] = route(f);
      pulse.position.set(x, 0.15, z);
    });
    const { eye, at } = getFocus(p);
    if (p > 0.59 && p < 0.71) {
      const glide = Math.sin(smooth(p, 0.59, 0.71) * Math.PI);
      eye.z -= glide * 2.8;
      at.z -= glide * 3.5;
    }
    camera.position.copy(eye);
    camera.lookAt(at);
    // A freestanding console follows a path between scene stops, not screen coordinates.
    const consolePositions = [
      [0, 1, 6.8],
      [0, 1, 5.3],
      [3.8, 1, 10],
      [3.8, 1, 7.8],
      [-0.8, 1, 5.6],
      [5.8, 1, 6.6],
      [2.7, 1, 5.2],
      [0, 0.95, 1.95],
    ];
    let k = 0;
    while (k < 6 && p > stops[k + 1]) k++;
    controller.position.copy(
      vector(consolePositions[k]).lerp(
        vector(consolePositions[k + 1]),
        smooth(p, stops[k], stops[k + 1]),
      ),
    );
    controller.rotation.y = k < 2 ? -0.12 : 0;
    controller.scale.setScalar(
      p > 0.9 ? 0.5 : p > 0.23 && p < 0.57 ? 1.6 : 0.9,
    );
    if (camera.aspect < 0.8) {
      controller.position.x = at.x;
      controller.scale.setScalar(p > 0.9 ? 0.5 : 0.78);
    }
    const mobileScreen = width < 600 ? smooth(p, 0.89, 0.97) : 0;
    rooms[0].monitorFrame.scale.y = mix(1, 1.48, mobileScreen);
    rooms[0].monitorFrame.position.y = mix(1.79, 2, mobileScreen);
    rooms[0].display.mesh.scale.y = mix(1, 1.48, mobileScreen);
    rooms[0].display.mesh.position.y = mix(1.79, 2, mobileScreen);
    screen.position.y = mix(1.79, 2, mobileScreen);
    terminal.style.height = `${(1000 * mix(0.88, 1.3, mobileScreen)) / 1.7}px`;
    terminal.classList.toggle("terminal-portrait", mobileScreen > 0.9);
    screen.visible = p > 0.94;
    terminal.style.visibility = screen.visible ? "visible" : "hidden";
    terminal.inert = !screen.visible;
    installKeys.visible = p > 0.92;
    paintText(p);
    if (p > 0.82 && p < 0.9) {
      helper.draw(
        zh
          ? "独立日志\n[info] 辅助服务就绪\n[info] 成员已连接\n日常连接：普通账户"
          : "SEPARATE LOGS\n[info] helper ready\n[info] peer connected\nCONNECT: YOUR ACCOUNT",
      );
    }
    if (p > 0.54 && p < 0.7) {
      const tick = state.reduced ? 0 : Math.floor(ms / 180);
      if (tick !== lastActivity) {
        lastActivity = tick;
        const displays = [rooms[1].display, rooms[3].display];
        displays.forEach((display, index) => {
          const c = display.canvas,
            ctx = c.getContext("2d");
          ctx.fillStyle = "#172b25";
          ctx.fillRect(0, 0, c.width, c.height);
          ctx.fillStyle = "#cdddba";
          ctx.font = "54px monospace";
          ctx.fillText(
            index
              ? zh
                ? "一起开玩 · 局域网"
                : "PLAY TOGETHER / LAN"
              : zh
                ? "一起创造 · 本地服务"
                : "BUILD / localhost:3000",
            40,
            65,
            930,
          );
          if (index) {
            ctx.strokeStyle = "#3d5540";
            ctx.lineWidth = 3;
            for (let x = 40; x < 990; x += 95) {
              ctx.beginPath();
              ctx.moveTo(x, 105);
              ctx.lineTo(x, c.height - 20);
              ctx.stroke();
            }
            for (let y = 105; y < c.height; y += 65) {
              ctx.beginPath();
              ctx.moveTo(40, y);
              ctx.lineTo(990, y);
              ctx.stroke();
            }
            for (let i = 0; i < 6; i++) {
              ctx.fillStyle = ["#dab778", "#bc7753", "#aac39c"][i % 3];
              ctx.beginPath();
              ctx.arc(
                130 + i * 145 + Math.sin(tick * 0.1 + i) * 30,
                220 + Math.cos(tick * 0.12 + i) * 80,
                22,
                0,
                Math.PI * 2,
              );
              ctx.fill();
            }
          } else {
            ctx.fillStyle = "#cabda0";
            ctx.fillRect(45, 110, 920, c.height - 150);
            ctx.fillStyle = "#536c53";
            ctx.fillRect(75, 145, 350, 155);
            ctx.fillStyle = "#e9dcc0";
            ctx.fillRect(450, 145, 480, 35);
            ctx.fillRect(450, 210, 330, 22);
            ctx.fillRect(450, 260, 390, 22);
            ctx.fillStyle = "#b96743";
            ctx.beginPath();
            ctx.moveTo(470 + Math.sin(tick * 0.09) * 100, 280);
            ctx.lineTo(500 + Math.sin(tick * 0.09) * 100, 330);
            ctx.lineTo(450 + Math.sin(tick * 0.09) * 100, 319);
            ctx.fill();
          }
          display.texture.needsUpdate = true;
        });
      }
    } else lastActivity = -1;
    if (Math.abs(p - shadowProgress) > .001 || Math.abs(night - shadowNight) > .004) {
      renderer.shadowMap.needsUpdate = true;
      shadowProgress = p;
      shadowNight = night;
    }
    renderer.render(scene, camera);
    css.render(scene, camera);
    // Semantic hit surfaces must respect WebGL occlusion and visibility.
    const origin = camera.position;
    if (ms - lastOcclusion > 120 || state.reduced) {
      lastOcclusion = ms;
      surfaces.forEach((item) => {
        let visible = true;
        for (let obj = item; obj; obj = obj.parent)
          if (!obj.visible) visible = false;
        if (visible) {
          const pos = item.getWorldPosition(new T.Vector3());
          const dir = pos.clone().sub(origin);
          ray.set(origin, dir.clone().normalize());
          ray.far = dir.length() - 0.025;
          const hits = ray.intersectObjects(occluders, false);
          visible = !hits.some((h) => {
            for (let o = h.object; o; o = o.parent)
              if (!o.visible) return false;
            return !h.object.material.transparent && h.distance < ray.far;
          });
        }
        item.element.style.visibility = visible ? "visible" : "hidden";
        item.element.inert = !visible;
      });
    }
    if (!document.hidden && !state.reduced)
      frame = requestAnimationFrame(render);
  }
  function refresh() {
    cancelAnimationFrame(frame);
    render();
  }
  function resize() {
    width = host.clientWidth;
    height = host.clientHeight;
    if (!width || !height) return;
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height);
    css.setSize(width, height);
    refresh();
  }
  const observer = new ResizeObserver(resize);
  observer.observe(host);
  const visibility = () => refresh();
  document.addEventListener("visibilitychange", visibility);
  const lost = (e) => {
    e.preventDefault();
    cancelAnimationFrame(frame);
    action("webgl-lost");
  };
  renderer.domElement.addEventListener("webglcontextlost", lost);
  resize();
  return {
    refresh,
    dispose() {
      disposed = true;
      cancelAnimationFrame(frame);
      observer.disconnect();
      document.removeEventListener("visibilitychange", visibility);
      renderer.domElement.removeEventListener("webglcontextlost", lost);
      surfaces.forEach((s) => s.removeFromParent());
      textures.forEach((t) => t.dispose());
      geo.forEach((g) => g.dispose());
      materials.forEach((m) => m.dispose());
      custom.forEach((m) => m.dispose());
      renderer.dispose();
      renderer.domElement.remove();
      css.domElement.remove();
    },
  };
}
