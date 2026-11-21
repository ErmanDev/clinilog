/**
 * Decorative login animation: a tiny low-poly school clinic that loops forever.
 *
 *  1. Students wait on chairs in arrival order.
 *  2. The next one walks to the front desk; the nurse turns to the computer and
 *     checks them in ("typing" bubble), then a green check pops up (approved).
 *  3. The student goes to the treatment room (waiting by the door if it's busy),
 *     sits on the bed and the doctor treats them (pulsing medical cross).
 *  4. They leave through the exit, and later walk back in and queue again.
 *
 * Plain Three.js, no assets: every shape is built from primitives, icons are drawn
 * on small canvases. Call `dispose()` to stop it and free all GPU resources.
 */
import * as THREE from "three";

export interface ClinicSceneHandle {
  dispose(): void;
}

// ---------- palette (Clinilog brand blue #6EC5D3 + calm neutrals) ----------
const C = {
  floor: 0xf1f8f7,
  floorSide: 0xc9e3df,
  tile: 0xe1efec,
  wall: 0xffffff,
  wallTop: 0xd6ebe8,
  desk: 0xf8fafc,
  deskTop: 0x6ec5d3,
  monitor: 0x334155,
  chair: 0x6ec5d3,
  chairLeg: 0x94a3b8,
  bedFrame: 0x94a3b8,
  sheet: 0xffffff,
  pillow: 0xdbeafe,
  doorway: 0x0f766e,
  plantPot: 0xe2e8f0,
  plant: 0x34d399,
  cabinet: 0xe2e8f0,
  nurseTop: 0x6ec5d3,
  nursePants: 0x4fa9b8,
  doctorTop: 0xffffff,
  doctorPants: 0x475569,
  hair: [0x1f1b16, 0x3b2a1a, 0x111111, 0x4a3426],
  skin: [0xf1c27d, 0xe0ac69, 0xc68642, 0xd9a066],
  uniformTop: 0xffffff,
  uniformBottom: [0x1e3a8a, 0x1f2937, 0x1e40af, 0x334155],
  bag: [0xef4444, 0xf59e0b, 0x10b981, 0x8b5cf6],
};

// ---------- layout (y = 0 is the floor; people face +z when rotation.y = 0) ----------
// Floor x -4.2..4.2, z -5.4..2.6 (taller than wide, to suit the portrait panel).
// Back-left: front desk. Back-right: treatment room. Front-left: waiting chairs.
// Entrance in the left wall, exit at the front of the right wall.
const V = (x: number, z: number) => new THREE.Vector3(x, 0, z);
const ENTRANCE = V(-3.75, -0.9);
const EXIT = V(3.75, 1.5);
const CHAIRS = [V(-3.45, 0.25), V(-3.45, 1.15), V(-3.45, 2.05)];
const DESK = V(-2.2, -3.55);
const DESK_SPOT = V(-2.2, -2.55);
const NURSE_SPOT = V(-2.2, -4.4);
const MONITOR_SPOT = V(-2.95, -3.7);
const ROOM_WAIT = V(1.0, -1.55);
const ROOM_DOOR_OUT = V(2.1, -1.75);
const ROOM_DOOR_IN = V(2.1, -3.0);
const BEDSIDE = V(3.05, -3.85);
const DOCTOR_SPOT = V(2.1, -3.85);

const WALK_SPEED = 1.55; // units per second
const CHECK_SECONDS = 2.2;
const APPROVE_SECONDS = 1.0;
const TREAT_SECONDS = 3.4;
const FADE_SECONDS = 0.5;

const angleTo = (from: THREE.Vector3, to: THREE.Vector3) => Math.atan2(to.x - from.x, to.z - from.z);
const lerpAngle = (a: number, b: number, t: number) => {
  let d = b - a;
  while (d > Math.PI) d -= Math.PI * 2;
  while (d < -Math.PI) d += Math.PI * 2;
  return a + d * t;
};

// ---------- people ----------

interface Person {
  root: THREE.Group;
  hips: THREE.Group;
  legL: THREE.Group;
  legR: THREE.Group;
  armL: THREE.Group;
  armR: THREE.Group;
  mats: THREE.MeshLambertMaterial[];
  shadowMat: THREE.MeshBasicMaterial;
  phase: number;
  sit: number; // 0 standing → 1 sitting
  heading: number;
}

function makePerson(opts: { top: number; bottom: number; skin: number; hair: number; bag?: number; cap?: boolean; coat?: boolean }): Person {
  const mats: THREE.MeshLambertMaterial[] = [];
  const mat = (color: number) => {
    const m = new THREE.MeshLambertMaterial({ color, transparent: true });
    mats.push(m);
    return m;
  };
  const root = new THREE.Group();
  const hips = new THREE.Group();
  root.add(hips);

  const limb = (w: number, h: number, d: number, color: number) => {
    const pivot = new THREE.Group();
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), mat(color));
    mesh.position.y = -h / 2;
    pivot.add(mesh);
    return pivot;
  };

  const legL = limb(0.15, 0.55, 0.16, opts.bottom);
  const legR = limb(0.15, 0.55, 0.16, opts.bottom);
  legL.position.set(-0.1, 0.55, 0);
  legR.position.set(0.1, 0.55, 0);
  hips.add(legL, legR);

  const torso = new THREE.Mesh(new THREE.CapsuleGeometry(0.21, opts.coat ? 0.42 : 0.32, 4, 10), mat(opts.top));
  torso.position.y = opts.coat ? 0.84 : 0.88;
  hips.add(torso);

  const armL = limb(0.1, 0.42, 0.1, opts.top);
  const armR = limb(0.1, 0.42, 0.1, opts.top);
  armL.position.set(-0.29, 1.1, 0);
  armR.position.set(0.29, 1.1, 0);
  hips.add(armL, armR);

  const head = new THREE.Mesh(new THREE.SphereGeometry(0.19, 16, 12), mat(opts.skin));
  head.position.y = 1.42;
  const hair = new THREE.Mesh(new THREE.SphereGeometry(0.205, 16, 8, 0, Math.PI * 2, 0, Math.PI / 2), mat(opts.hair));
  hair.position.y = 1.45;
  hair.rotation.x = -0.25;
  hips.add(head, hair);

  if (opts.cap) {
    const cap = new THREE.Mesh(new THREE.CylinderGeometry(0.12, 0.15, 0.09, 14), mat(0xffffff));
    cap.position.y = 1.64;
    hips.add(cap);
  }
  if (opts.bag !== undefined) {
    const bag = new THREE.Mesh(new THREE.BoxGeometry(0.32, 0.36, 0.14), mat(opts.bag));
    bag.position.set(0, 0.92, -0.25);
    hips.add(bag);
  }

  const shadowMat = new THREE.MeshBasicMaterial({ color: 0x0f172a, transparent: true, opacity: 0.14, depthWrite: false });
  const shadow = new THREE.Mesh(new THREE.CircleGeometry(0.32, 20), shadowMat);
  shadow.rotation.x = -Math.PI / 2;
  shadow.position.y = 0.012;
  root.add(shadow);

  return { root, hips, legL, legR, armL, armR, mats, shadowMat, phase: 0, sit: 0, heading: 0 };
}

function setOpacity(p: Person, a: number) {
  for (const m of p.mats) m.opacity = a;
  p.shadowMat.opacity = 0.14 * a;
  p.root.visible = a > 0.01;
}

/** Walk cycle, sitting and arm poses. `busy` makes the arms move (typing / treating). */
function pose(p: Person, dt: number, moving: boolean, sitTarget: number, busy = false) {
  p.sit += (sitTarget - p.sit) * Math.min(1, dt * 6);
  if (moving) p.phase += dt * 9;
  const swing = moving ? Math.sin(p.phase) : 0;
  const s = p.sit;
  p.hips.position.y = -0.27 * s + (moving ? Math.abs(Math.sin(p.phase)) * 0.035 : 0);
  p.legL.rotation.x = swing * 0.6 * (1 - s) - (Math.PI / 2) * s;
  p.legR.rotation.x = -swing * 0.6 * (1 - s) - (Math.PI / 2) * s;
  const work = busy ? Math.sin(performance.now() / 90) * 0.15 : 0;
  p.armL.rotation.x = -swing * 0.5 - 0.25 * s - (busy ? 0.9 + work : 0);
  p.armR.rotation.x = swing * 0.5 - 0.25 * s - (busy ? 0.9 - work : 0);
}

// ---------- icons (drawn once on canvases) ----------

function iconTexture(draw: (g: CanvasRenderingContext2D) => void) {
  const c = document.createElement("canvas");
  c.width = c.height = 128;
  const g = c.getContext("2d")!;
  draw(g);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

const circle = (g: CanvasRenderingContext2D, fill: string, stroke?: string) => {
  g.beginPath();
  g.arc(64, 64, 56, 0, Math.PI * 2);
  g.fillStyle = fill;
  g.fill();
  if (stroke) {
    g.lineWidth = 8;
    g.strokeStyle = stroke;
    g.stroke();
  }
};

function makeIcons() {
  const check = iconTexture((g) => {
    circle(g, "#16a34a", "#ffffff");
    g.strokeStyle = "#ffffff";
    g.lineWidth = 14;
    g.lineCap = "round";
    g.lineJoin = "round";
    g.beginPath();
    g.moveTo(38, 66);
    g.lineTo(56, 84);
    g.lineTo(90, 46);
    g.stroke();
  });
  const cross = iconTexture((g) => {
    circle(g, "#ffffff", "#6ec5d3");
    g.fillStyle = "#e11d48";
    g.fillRect(52, 30, 24, 68);
    g.fillRect(30, 52, 68, 24);
  });
  // "Checking on the computer": a little screen with 1–3 typing dots.
  const typing = [1, 2, 3].map((n) =>
    iconTexture((g) => {
      g.fillStyle = "#ffffff";
      g.beginPath();
      g.roundRect(10, 24, 108, 74, 14);
      g.fill();
      g.fillStyle = "#6ec5d3";
      g.beginPath();
      g.roundRect(18, 32, 92, 58, 8);
      g.fill();
      g.fillStyle = "#ffffff";
      for (let i = 0; i < n; i++) {
        g.beginPath();
        g.arc(44 + i * 20, 61, 7, 0, Math.PI * 2);
        g.fill();
      }
      g.fillStyle = "#ffffff";
      g.fillRect(56, 98, 16, 10);
    }),
  );
  return { check, cross, typing };
}

function sprite(map: THREE.Texture, size: number) {
  const m = new THREE.SpriteMaterial({ map, transparent: true, depthTest: false });
  const s = new THREE.Sprite(m);
  s.scale.setScalar(size);
  s.renderOrder = 10;
  s.visible = false;
  return s;
}

// ---------- room furniture ----------

function buildClinic(scene: THREE.Scene, own: (o: THREE.Object3D) => void) {
  const lam = (color: number) => new THREE.MeshLambertMaterial({ color });
  const box = (w: number, h: number, d: number, color: number, x: number, y: number, z: number) => {
    const m = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), lam(color));
    m.position.set(x, y, z);
    scene.add(m);
    own(m);
    return m;
  };

  // Floor slab with a visible edge, plus soft tile lines.
  box(8.6, 0.3, 8.2, C.floorSide, 0, -0.16, -1.4);
  box(8.4, 0.02, 8.0, C.floor, 0, 0, -1.4);
  for (let x = -3.15; x <= 3.2; x += 1.05) box(0.02, 0.005, 7.9, C.tile, x, 0.012, -1.4);
  for (let z = -4.35; z <= 2.2; z += 1.05) box(8.3, 0.005, 0.02, C.tile, 0, 0.012, z);

  // Walls (low, cut-away so the camera sees inside). Gaps are doors.
  const H = 1.3;
  const wall = (x0: number, z0: number, x1: number, z1: number, h = H) => {
    const len = Math.hypot(x1 - x0, z1 - z0);
    const m = new THREE.Mesh(new THREE.BoxGeometry(len, h, 0.12), lam(C.wall));
    m.position.set((x0 + x1) / 2, h / 2, (z0 + z1) / 2);
    m.rotation.y = -Math.atan2(z1 - z0, x1 - x0);
    scene.add(m);
    own(m);
    const top = new THREE.Mesh(new THREE.BoxGeometry(len, 0.04, 0.14), lam(C.wallTop));
    top.position.set(m.position.x, h + 0.02, m.position.z);
    top.rotation.y = m.rotation.y;
    scene.add(top);
    own(top);
  };
  wall(-4.2, -5.4, 4.2, -5.4); // back
  wall(-4.2, -5.4, -4.2, -1.45); // left, up to the entrance
  wall(-4.2, -0.35, -4.2, 2.6);
  wall(4.2, -5.4, 4.2, 0.95); // right, up to the exit
  wall(4.2, 2.05, 4.2, 2.6);
  wall(0.6, -5.4, 0.6, -2.4); // treatment room side
  wall(0.6, -2.4, 1.55, -2.4, 0.55); // room front, low so we can look in
  wall(2.65, -2.4, 4.2, -2.4, 0.55);

  // Doorways: dark panels in the wall gaps, with a frame.
  const doorway = (x: number, z: number) => {
    box(0.06, 1.25, 1.1, C.doorway, x, 0.63, z);
    box(0.14, 0.08, 1.3, C.wallTop, x, 1.3, z);
  };
  doorway(-4.22, -0.9);
  doorway(4.22, 1.5);

  // Waiting chairs (facing +x, backs to the left wall).
  for (const c of CHAIRS) {
    box(0.5, 0.08, 0.5, C.chair, c.x - 0.12, 0.42, c.z);
    box(0.08, 0.5, 0.5, C.chair, c.x - 0.4, 0.68, c.z);
    for (const dz of [-0.2, 0.2]) box(0.06, 0.42, 0.06, C.chairLeg, c.x - 0.12, 0.21, c.z + dz);
  }

  // Front desk with a computer.
  box(2.1, 0.78, 0.7, C.desk, DESK.x, 0.39, DESK.z);
  box(2.16, 0.06, 0.76, C.deskTop, DESK.x, 0.8, DESK.z);
  const monitor = box(0.55, 0.38, 0.06, C.monitor, MONITOR_SPOT.x, 1.08, MONITOR_SPOT.z);
  monitor.rotation.y = angleTo(MONITOR_SPOT, NURSE_SPOT) + Math.PI;
  box(0.08, 0.16, 0.08, C.monitor, MONITOR_SPOT.x, 0.88, MONITOR_SPOT.z);
  box(0.5, 0.03, 0.18, 0xe2e8f0, DESK.x + 0.1, 0.84, DESK.z - 0.12);

  // Treatment room: bed along the back, pillow, cabinet with a cross.
  box(0.95, 0.42, 1.9, C.bedFrame, 3.5, 0.21, -4.15);
  box(0.9, 0.12, 1.85, C.sheet, 3.5, 0.48, -4.15);
  box(0.7, 0.1, 0.4, C.pillow, 3.5, 0.59, -4.9);
  box(0.4, 0.9, 0.9, C.cabinet, 0.95, 0.45, -4.75);
  box(0.02, 0.07, 0.24, 0xe11d48, 1.16, 0.72, -4.75);
  box(0.02, 0.24, 0.07, 0xe11d48, 1.16, 0.72, -4.75);

  // Plants for a bit of life.
  const plant = (x: number, z: number) => {
    const pot = new THREE.Mesh(new THREE.CylinderGeometry(0.2, 0.16, 0.34, 14), lam(C.plantPot));
    pot.position.set(x, 0.17, z);
    const leaves = new THREE.Mesh(new THREE.IcosahedronGeometry(0.32, 0), lam(C.plant));
    leaves.position.set(x, 0.58, z);
    scene.add(pot, leaves);
    own(pot);
    own(leaves);
  };
  plant(-3.8, -5.0);
  plant(0.15, -5.0);
  plant(3.8, 2.2);
}

// ---------- students ----------

type State =
  | { kind: "offstage"; wait: number }
  | { kind: "fadeIn" }
  | { kind: "toChair" }
  | { kind: "seated" }
  | { kind: "toDesk" }
  | { kind: "checking"; t: number }
  | { kind: "approved"; t: number }
  | { kind: "toRoomWait" }
  | { kind: "roomWait" }
  | { kind: "toBed" }
  | { kind: "treating"; t: number }
  | { kind: "leaving" }
  | { kind: "fadeOut"; t: number };

interface Student {
  id: number;
  p: Person;
  state: State;
  path: THREE.Vector3[];
  chair: number;
  face: number | null; // heading to turn to when standing still
  sitTarget: number;
  alpha: number;
}

export function startClinicScene(container: HTMLElement, opts: { reducedMotion: boolean }): ClinicSceneHandle {
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.domElement.setAttribute("aria-hidden", "true");
  renderer.domElement.style.display = "block";
  container.appendChild(renderer.domElement);

  const scene = new THREE.Scene();
  const owned: THREE.Object3D[] = [];
  const own = (o: THREE.Object3D) => owned.push(o);

  scene.add(new THREE.HemisphereLight(0xffffff, 0xb8d8d4, 2.2));
  const sun = new THREE.DirectionalLight(0xffffff, 1.4);
  sun.position.set(4, 10, 6);
  scene.add(sun);

  buildClinic(scene, own);
  const icons = makeIcons();

  // Staff.
  const nurse = makePerson({ top: C.nurseTop, bottom: C.nursePants, skin: C.skin[1], hair: C.hair[0], cap: true });
  nurse.root.position.copy(NURSE_SPOT);
  const doctor = makePerson({ top: C.doctorTop, bottom: C.doctorPants, skin: C.skin[2], hair: C.hair[2], coat: true });
  doctor.root.position.copy(DOCTOR_SPOT);
  doctor.heading = angleTo(DOCTOR_SPOT, BEDSIDE);
  doctor.root.rotation.y = doctor.heading;
  scene.add(nurse.root, doctor.root);

  const typingIcon = sprite(icons.typing[0], 0.55);
  typingIcon.position.set(MONITOR_SPOT.x, 1.75, MONITOR_SPOT.z);
  const checkIcon = sprite(icons.check, 0.5);
  const crossIcon = sprite(icons.cross, 0.5);
  scene.add(typingIcon, checkIcon, crossIcon);

  // Students.
  const students: Student[] = [0, 1, 2, 3].map((i) => {
    const p = makePerson({
      top: C.uniformTop,
      bottom: C.uniformBottom[i],
      skin: C.skin[i],
      hair: C.hair[i],
      bag: C.bag[i],
    });
    scene.add(p.root);
    return { id: i, p, state: { kind: "offstage", wait: 0 }, path: [], chair: -1, face: null, sitTarget: 0, alpha: 1 };
  });

  const chairTaken: (number | null)[] = [null, null, null];
  const queue: number[] = [];
  let deskBusy: number | null = null;
  let roomBusy: number | null = null;
  let roomWaitBusy: number | null = null;

  // Start: three students seated in line, the fourth arriving soon.
  for (let i = 0; i < 3; i++) {
    const s = students[i];
    s.chair = i;
    chairTaken[i] = s.id;
    s.p.root.position.copy(CHAIRS[i]);
    s.p.heading = s.p.root.rotation.y = Math.PI / 2;
    s.p.sit = s.sitTarget = 1;
    s.state = { kind: "seated" };
    queue.push(s.id);
  }
  students[3].state = { kind: "offstage", wait: 4 };
  setOpacity(students[3].p, 0);
  students[3].alpha = 0;

  const go = (s: Student, path: THREE.Vector3[], next: State, face: number | null = null) => {
    s.path = path.map((v) => v.clone());
    s.state = next;
    s.face = face;
    s.sitTarget = 0;
  };

  function step(dt: number) {
    const time = performance.now() / 1000;

    for (const s of students) {
      const p = s.p;
      let moving = false;

      // Follow the current path.
      if (s.path.length && p.sit < 0.15) {
        const target = s.path[0];
        const pos = p.root.position;
        const dist = Math.hypot(target.x - pos.x, target.z - pos.z);
        const stepLen = WALK_SPEED * dt;
        if (dist <= stepLen) {
          pos.x = target.x;
          pos.z = target.z;
          s.path.shift();
        } else {
          pos.x += ((target.x - pos.x) / dist) * stepLen;
          pos.z += ((target.z - pos.z) / dist) * stepLen;
          moving = true;
        }
        p.heading = lerpAngle(p.heading, Math.atan2(target.x - pos.x, target.z - pos.z), Math.min(1, dt * 10));
      } else if (s.face !== null) {
        p.heading = lerpAngle(p.heading, s.face, Math.min(1, dt * 6));
      }
      p.root.rotation.y = p.heading;
      const arrived = s.path.length === 0;
      const st = s.state;

      switch (st.kind) {
        case "offstage":
          st.wait -= dt;
          if (st.wait <= 0) {
            const free = chairTaken.findIndex((c) => c === null);
            if (free >= 0) {
              s.chair = free;
              chairTaken[free] = s.id;
              p.root.position.copy(ENTRANCE);
              p.heading = Math.PI / 2;
              s.state = { kind: "fadeIn" };
            } else {
              st.wait = 1;
            }
          }
          break;
        case "fadeIn":
          s.alpha = Math.min(1, s.alpha + dt / FADE_SECONDS);
          if (s.alpha >= 1) {
            const c = CHAIRS[s.chair];
            go(s, [V(-2.75, -0.7), V(-2.75, c.z), c], { kind: "toChair" }, Math.PI / 2);
          }
          break;
        case "toChair":
          if (arrived) {
            s.state = { kind: "seated" };
            s.sitTarget = 1;
            queue.push(s.id);
          }
          break;
        case "seated":
          s.sitTarget = 1;
          if (deskBusy === null && queue[0] === s.id) {
            queue.shift();
            deskBusy = s.id;
            chairTaken[s.chair] = null;
            const c = CHAIRS[s.chair];
            s.chair = -1;
            go(s, [V(c.x + 0.75, c.z), V(-2.2, -1.5), DESK_SPOT], { kind: "toDesk" }, Math.PI);
          }
          break;
        case "toDesk":
          if (arrived) s.state = { kind: "checking", t: 0 };
          break;
        case "checking":
          st.t += dt;
          if (st.t >= CHECK_SECONDS) {
            s.state = { kind: "approved", t: 0 };
            checkIcon.visible = true;
          }
          break;
        case "approved":
          st.t += dt;
          checkIcon.position.set(p.root.position.x, 1.95 + Math.min(st.t, 0.3) * 0.5, p.root.position.z);
          checkIcon.scale.setScalar(0.5 * Math.min(1, 0.4 + st.t * 4));
          if (st.t >= APPROVE_SECONDS && roomWaitBusy === null) {
            checkIcon.visible = false;
            deskBusy = null;
            roomWaitBusy = s.id;
            go(s, [V(-1.1, -1.7), ROOM_WAIT], { kind: "toRoomWait" }, angleTo(ROOM_WAIT, ROOM_DOOR_OUT));
          }
          break;
        case "toRoomWait":
          if (arrived) s.state = { kind: "roomWait" };
          break;
        case "roomWait":
          if (roomBusy === null) {
            roomBusy = s.id;
            roomWaitBusy = null;
            go(s, [ROOM_DOOR_OUT, ROOM_DOOR_IN, V(2.6, -3.4), BEDSIDE], { kind: "toBed" }, -Math.PI / 2);
          }
          break;
        case "toBed":
          if (arrived) {
            s.state = { kind: "treating", t: 0 };
            s.sitTarget = 1;
            crossIcon.visible = true;
          }
          break;
        case "treating":
          st.t += dt;
          crossIcon.position.set(BEDSIDE.x + 0.15, 2.0, BEDSIDE.z);
          crossIcon.scale.setScalar(0.45 + Math.sin(st.t * 5) * 0.05);
          if (st.t >= TREAT_SECONDS) {
            crossIcon.visible = false;
            go(s, [V(2.6, -3.4), ROOM_DOOR_IN, ROOM_DOOR_OUT, V(3.0, 0.9), EXIT], { kind: "leaving" });
          }
          break;
        case "leaving":
          // The room is free once they're back out of its door.
          if (roomBusy === s.id && s.path.length <= 2) roomBusy = null;
          if (arrived) s.state = { kind: "fadeOut", t: 0 };
          break;
        case "fadeOut":
          s.alpha = Math.max(0, s.alpha - dt / FADE_SECONDS);
          if (s.alpha <= 0) s.state = { kind: "offstage", wait: 1.2 };
          break;
      }

      p.root.position.y = st.kind === "treating" || (st.kind === "toBed" && arrived) ? 0.24 * p.sit : 0;
      setOpacity(p, s.alpha);
      pose(p, dt, moving, s.sitTarget);
    }

    // Nurse: turns to the computer and types while checking a student in.
    const checking = students.some((s) => s.state.kind === "checking");
    nurse.heading = lerpAngle(nurse.heading, checking ? angleTo(NURSE_SPOT, MONITOR_SPOT) : 0, Math.min(1, dt * 5));
    nurse.root.rotation.y = nurse.heading;
    pose(nurse, dt, false, 0, checking);
    typingIcon.visible = checking;
    if (checking) typingIcon.material.map = icons.typing[Math.floor(time * 3) % 3];

    // Doctor: works while someone is being treated, otherwise idles.
    const treating = students.some((s) => s.state.kind === "treating");
    pose(doctor, dt, false, 0, treating);
    doctor.hips.position.y = treating ? Math.abs(Math.sin(time * 3)) * 0.03 : 0;
  }

  // ---------- camera: isometric-style, fitted to the panel ----------
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 100);
  const target = new THREE.Vector3(0, 0.4, -1.4);
  camera.position.copy(target).add(new THREE.Vector3(6.5, 11, 10));
  camera.lookAt(target);
  camera.updateMatrixWorld();

  const bounds = new THREE.Box3(new THREE.Vector3(-4.3, -0.3, -5.5), new THREE.Vector3(4.3, 1.8, 2.7));
  function fit() {
    const w = container.clientWidth || 1;
    const h = container.clientHeight || 1;
    renderer.setSize(w, h, false);
    renderer.domElement.style.width = "100%";
    renderer.domElement.style.height = "100%";
    // Project the scene's corners onto the camera's right/up axes.
    const right = new THREE.Vector3().setFromMatrixColumn(camera.matrixWorld, 0);
    const up = new THREE.Vector3().setFromMatrixColumn(camera.matrixWorld, 1);
    let minR = Infinity, maxR = -Infinity, minU = Infinity, maxU = -Infinity;
    for (const x of [bounds.min.x, bounds.max.x])
      for (const y of [bounds.min.y, bounds.max.y])
        for (const z of [bounds.min.z, bounds.max.z]) {
          const d = new THREE.Vector3(x, y, z).sub(target);
          const r = d.dot(right);
          const u = d.dot(up);
          minR = Math.min(minR, r);
          maxR = Math.max(maxR, r);
          minU = Math.min(minU, u);
          maxU = Math.max(maxU, u);
        }
    // Leave room above for the logo and below for the tagline.
    const aspect = w / h;
    const viewH = Math.max((maxU - minU) / 0.62, (maxR - minR) / 0.96 / aspect);
    const viewW = viewH * aspect;
    const cR = (minR + maxR) / 2;
    const cU = (minU + maxU) / 2;
    camera.left = cR - viewW / 2;
    camera.right = cR + viewW / 2;
    camera.top = cU + viewH * 0.47;
    camera.bottom = camera.top - viewH;
    camera.updateProjectionMatrix();
  }
  fit();
  const ro = new ResizeObserver(() => {
    fit();
    renderer.render(scene, camera);
  });
  ro.observe(container);

  // ---------- run ----------
  let frame = 0;
  if (opts.reducedMotion) {
    // Settle into a representative moment, then draw one still frame.
    for (let i = 0; i < 14 * 30; i++) step(1 / 30);
    renderer.render(scene, camera);
  } else {
    // 30 fps is plenty for this and halves the work on older clinic PCs.
    // (requestAnimationFrame also stops by itself while the window is minimized.)
    const FRAME = 1 / 30;
    const clock = new THREE.Clock();
    let pending = 0;
    const loop = () => {
      frame = requestAnimationFrame(loop);
      pending += clock.getDelta();
      if (pending < FRAME) return;
      step(Math.min(pending, 0.1));
      pending = 0;
      renderer.render(scene, camera);
    };
    loop();
  }

  return {
    dispose() {
      cancelAnimationFrame(frame);
      ro.disconnect();
      scene.traverse((o) => {
        const mesh = o as THREE.Mesh;
        if (mesh.geometry) mesh.geometry.dispose();
        const m = (mesh as { material?: THREE.Material | THREE.Material[] }).material;
        if (Array.isArray(m)) m.forEach((x) => x.dispose());
        else m?.dispose();
      });
      [icons.check, icons.cross, ...icons.typing].forEach((t) => t.dispose());
      renderer.dispose();
      renderer.forceContextLoss();
      renderer.domElement.remove();
      owned.length = 0;
    },
  };
}
