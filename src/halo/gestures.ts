// H.A.L.O.: máquina de estados de gestos.
//
// Entrada: los 21 puntos por mano que da MediaPipe (coordenadas normalizadas de la
// imagen de la cámara, sin espejo). Salida: acciones para el backend y un estado para
// el HUD. Es código puro (sin DOM ni Tauri) para poder probarlo con tests.

export interface Pt {
  x: number;
  y: number;
  z?: number;
}
export type Hand = Pt[];

export type Pose = "none" | "point" | "pinch" | "open" | "fist" | "other";

export type HaloAction =
  | { kind: "pointer"; x: number; y: number }
  | { kind: "button"; action: "down" | "up" | "click" | "right_click" }
  | { kind: "window"; action: "maximize" | "restore" | "minimize" | "snap_left" | "snap_right" }
  | { kind: "desktop"; dir: "left" | "right" }
  | { kind: "stop" }
  | { kind: "resume" };

export interface HudState {
  pose: Pose;
  hands: number;
  /** Cursor en coordenadas de pantalla normalizadas, si el gesto lo mueve. */
  cursor: { x: number; y: number } | null;
  /** Texto corto para el HUD («Arrastrando», «Minimizar…»). */
  label: string;
  /** Progreso 0..1 de los gestos que se mantienen (puño, palma quieta). */
  progress: number;
  dragging: boolean;
  paused: boolean;
}

export interface Config {
  /** Relación ancho/alto de la imagen, para medir distancias reales. */
  aspect: number;
  pinchOn: number;
  pinchOff: number;
  /** Zona activa de la cámara que se mapea a toda la pantalla. */
  zone: { x0: number; x1: number; y0: number; y1: number };
  freezeMs: number;
  dragMove: number;
  dragHoldMs: number;
  swipeDist: number;
  swipeMs: number;
  swipeCooldownMs: number;
  fistHoldMs: number;
  stopHoldMs: number;
  stopStill: number;
  lostMs: number;
  deadZone: number;
}

export const defaultConfig: Config = {
  aspect: 4 / 3,
  pinchOn: 0.28,
  pinchOff: 0.42,
  zone: { x0: 0.2, x1: 0.8, y0: 0.15, y1: 0.7 },
  freezeMs: 150,
  dragMove: 0.03,
  dragHoldMs: 450,
  swipeDist: 0.25,
  swipeMs: 250,
  swipeCooldownMs: 1000,
  fistHoldMs: 1000,
  stopHoldMs: 1200,
  stopStill: 0.04,
  lostMs: 300,
  deadZone: 0.0015,
};

// ---------- Geometría ----------

const WRIST = 0;
const THUMB_TIP = 4;
const INDEX_MCP = 5;
const INDEX_TIP = 8;
const MIDDLE_MCP = 9;
const MIDDLE_TIP = 12;
const PINKY_MCP = 17;
const FINGERS: [tip: number, pip: number][] = [
  [8, 6],
  [12, 10],
  [16, 14],
  [20, 18],
];

/** Pasa a espejo (como un espejo: tu derecha es la derecha de la pantalla). */
export function mirror(p: Pt): Pt {
  return { x: 1 - p.x, y: p.y, z: p.z };
}

function dist(a: Pt, b: Pt, aspect: number): number {
  return Math.hypot((a.x - b.x) * aspect, a.y - b.y);
}

export interface HandFeatures {
  size: number;
  extended: boolean[];
  pinch: number;
  middlePinch: number;
  palm: Pt;
  /** Punto estable para el cursor: nudillos de índice y corazón (no se mueve al pellizcar). */
  anchor: Pt;
  pinchPoint: Pt;
}

export function features(h: Hand, aspect: number): HandFeatures {
  const size = Math.max(dist(h[WRIST], h[MIDDLE_MCP], aspect), 1e-4);
  const extended = FINGERS.map(([tip, pip]) => dist(h[WRIST], h[tip], aspect) > dist(h[WRIST], h[pip], aspect) * 1.12);
  const avg = (...ids: number[]): Pt => ({
    x: ids.reduce((s, i) => s + h[i].x, 0) / ids.length,
    y: ids.reduce((s, i) => s + h[i].y, 0) / ids.length,
  });
  // En un puño la punta del índice queda pegada a la palma: eso no es un pellizco.
  const indexOut = dist(h[WRIST], h[INDEX_TIP], aspect) > dist(h[WRIST], h[INDEX_MCP], aspect) * 1.15;
  return {
    size,
    extended,
    pinch: indexOut ? dist(h[THUMB_TIP], h[INDEX_TIP], aspect) / size : 9,
    middlePinch: dist(h[THUMB_TIP], h[MIDDLE_TIP], aspect) / size,
    palm: avg(WRIST, INDEX_MCP, MIDDLE_MCP, PINKY_MCP),
    anchor: avg(INDEX_MCP, MIDDLE_MCP),
    pinchPoint: avg(THUMB_TIP, INDEX_TIP),
  };
}

export function classify(f: HandFeatures, pinching: boolean): Pose {
  const [index, middle, ring, pinky] = f.extended;
  if (pinching) return "pinch";
  if (index && middle && ring && pinky) return "open";
  if (!index && !middle && !ring && !pinky) return "fist";
  if (index && !middle && !ring && !pinky) return "point";
  return "other";
}

// ---------- Suavizado (One Euro filter) ----------

class LowPass {
  private y: number | null = null;
  filter(x: number, alpha: number): number {
    this.y = this.y == null ? x : alpha * x + (1 - alpha) * this.y;
    return this.y;
  }
  reset() {
    this.y = null;
  }
  get value() {
    return this.y;
  }
}

export class OneEuro {
  private x = new LowPass();
  private dx = new LowPass();
  private last: number | null = null;
  constructor(
    private minCutoff = 1.0,
    private beta = 3.0,
    private dCutoff = 1.0,
  ) {}
  private alpha(cutoff: number, dt: number) {
    const tau = 1 / (2 * Math.PI * cutoff);
    return 1 / (1 + tau / dt);
  }
  filter(value: number, tMs: number): number {
    const prev = this.x.value;
    const dt = this.last == null ? 1 / 30 : Math.max((tMs - this.last) / 1000, 1e-3);
    this.last = tMs;
    const d = prev == null ? 0 : (value - prev) / dt;
    const ed = this.dx.filter(d, this.alpha(this.dCutoff, dt));
    const cutoff = this.minCutoff + this.beta * Math.abs(ed);
    return this.x.filter(value, this.alpha(cutoff, dt));
  }
  reset() {
    this.x.reset();
    this.dx.reset();
    this.last = null;
  }
}

// ---------- Máquina de estados ----------

type PinchState = { phase: "idle" } | { phase: "pending"; t0: number; start: Pt } | { phase: "drag" };

interface Bimanual {
  d0: number;
  c0: Pt;
  fired: boolean;
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

export class GestureEngine {
  private cfg: Config;
  private pinching = [false, false];
  private pinch: PinchState = { phase: "idle" };
  private freezeUntil = 0;
  private fx = new OneEuro();
  private fy = new OneEuro();
  private cursor: Pt | null = null;
  private emitted: Pt | null = null;
  private lastSeen = 0;
  private bi: Bimanual | null = null;
  private biLatch = false;
  private middleLatch = false;
  private swipeTrail: { t: number; x: number }[] = [];
  private swipeUntil = 0;
  private fistSince: number | null = null;
  private fistLatch = false;
  private still: { t0: number; p: Pt } | null = null;
  private stopLatch = false;
  private paused = false;
  private pose: Pose = "none";
  private label = "";
  private progress = 0;
  private handsSeen = 0;

  constructor(cfg: Partial<Config> = {}) {
    this.cfg = { ...defaultConfig, ...cfg };
  }

  get hud(): HudState {
    return {
      pose: this.pose,
      hands: this.handsSeen,
      cursor: this.cursor,
      label: this.label,
      progress: this.progress,
      dragging: this.pinch.phase === "drag",
      paused: this.paused,
    };
  }

  /** Procesa un fotograma. `hands` en coordenadas de la cámara (sin espejo). */
  update(rawHands: Hand[], t: number): HaloAction[] {
    const out: HaloAction[] = [];
    const c = this.cfg;
    const hands = rawHands.filter((h) => h.length >= 21).map((h) => h.map(mirror));
    this.handsSeen = hands.length;
    this.label = "";
    this.progress = 0;

    if (hands.length === 0) {
      if (t - this.lastSeen > c.lostMs) this.lost(out);
      this.pose = "none";
      return out;
    }
    this.lastSeen = t;

    const feats = hands.map((h) => features(h, c.aspect));
    // Histéresis del pellizco por mano (las manos llegan ordenadas por MediaPipe).
    feats.forEach((f, i) => {
      if (i > 1) return;
      this.pinching[i] = this.pinching[i] ? f.pinch < c.pinchOff : f.pinch < c.pinchOn;
    });
    if (feats.length < 2) this.pinching[1] = false;

    // Pausa: solo la palma quieta la reanuda.
    if (this.paused) {
      const pose = classify(feats[0], false);
      this.pose = pose;
      this.label = "En pausa · palma quieta para seguir";
      if (this.holdStill(pose === "open", feats[0].palm, t)) {
        this.paused = false;
        out.push({ kind: "resume" });
      }
      return out;
    }

    // ----- Dos manos pellizcando: colocar la ventana -----
    if (feats.length >= 2 && this.pinching[0] && this.pinching[1]) {
      this.endPinch(out, false);
      this.pose = "pinch";
      const a = feats[0].pinchPoint;
      const b = feats[1].pinchPoint;
      const d = dist(a, b, c.aspect);
      const center = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      if (!this.bi && !this.biLatch) this.bi = { d0: Math.max(d, 1e-3), c0: center, fired: false };
      if (this.bi && !this.bi.fired) {
        const ratio = d / this.bi.d0;
        const dx = center.x - this.bi.c0.x;
        let action: Extract<HaloAction, { kind: "window" }>["action"] | null = null;
        if (ratio > 1.5) action = "maximize";
        else if (ratio < 0.65) action = "restore";
        else if (dx > 0.15) action = "snap_right";
        else if (dx < -0.15) action = "snap_left";
        this.label = "Dos manos: separa, junta o desplaza";
        if (action) {
          out.push({ kind: "window", action });
          this.bi.fired = true;
          this.biLatch = true;
          this.label = windowLabel[action];
        }
      } else if (this.bi?.fired) {
        this.label = "Suelta las dos manos";
      }
      return out;
    }
    if (!(this.pinching[0] && this.pinching[1])) {
      this.bi = null;
      if (!this.pinching[0] && !this.pinching[1]) this.biLatch = false;
    }
    // Mientras quede una mano del gesto de dos manos pellizcando, no hacemos clic.
    if (this.biLatch) {
      this.pose = "pinch";
      this.label = "Suelta las dos manos";
      return out;
    }

    // ----- Una mano (la primera) -----
    const f = feats[0];
    const pose = classify(f, this.pinching[0]);
    this.pose = pose;

    // Puntero: índice estirado o pellizco.
    if (pose === "point" || pose === "pinch") {
      this.moveCursor(f.anchor, t, out);
    } else {
      this.fx.reset();
      this.fy.reset();
    }

    // Pellizco: clic o arrastre.
    if (pose === "pinch") {
      if (this.pinch.phase === "idle") {
        this.pinch = { phase: "pending", t0: t, start: this.cursor ?? { x: 0.5, y: 0.5 } };
        this.freezeUntil = t + c.freezeMs;
      } else if (this.pinch.phase === "pending") {
        const moved = this.cursor ? Math.hypot(this.cursor.x - this.pinch.start.x, this.cursor.y - this.pinch.start.y) : 0;
        if (moved > c.dragMove || t - this.pinch.t0 > c.dragHoldMs) {
          out.push({ kind: "button", action: "down" });
          this.pinch = { phase: "drag" };
        }
      }
      this.label = this.pinch.phase === "drag" ? "Arrastrando" : "Pellizco";
    } else {
      this.endPinch(out, true);
    }

    // Clic derecho: pulgar con corazón (con el índice estirado).
    const middlePinch = !this.pinching[0] && f.middlePinch < c.pinchOn && f.extended[0];
    if (middlePinch && !this.middleLatch) {
      out.push({ kind: "button", action: "right_click" });
      this.label = "Clic derecho";
    }
    this.middleLatch = middlePinch || (this.middleLatch && f.middlePinch < c.pinchOff);

    // Palma abierta: deslizar rápido (escritorios) o mantener quieta (alto).
    if (pose === "open") {
      this.swipeTrail.push({ t, x: f.palm.x });
      this.swipeTrail = this.swipeTrail.filter((s) => t - s.t <= c.swipeMs);
      const first = this.swipeTrail[0];
      const dx = f.palm.x - first.x;
      if (t >= this.swipeUntil && Math.abs(dx) > c.swipeDist) {
        // Como en el trackpad: mano hacia la izquierda → escritorio de la derecha.
        out.push({ kind: "desktop", dir: dx < 0 ? "right" : "left" });
        this.swipeUntil = t + c.swipeCooldownMs;
        this.swipeTrail = [];
        this.still = null;
        this.label = dx < 0 ? "Escritorio →" : "← Escritorio";
      } else if (t >= this.swipeUntil && this.holdStill(true, f.palm, t)) {
        this.paused = true;
        this.endPinch(out, false);
        out.push({ kind: "stop" });
        this.label = "Alto: escucha y gestos en pausa";
      } else if (!this.label) {
        this.label = this.progress > 0.15 ? "Mantén la palma para pausar" : "";
      }
    } else {
      this.swipeTrail = [];
      this.holdStill(false, f.palm, t);
    }

    // Puño mantenido: minimizar la ventana activa.
    if (pose === "fist") {
      if (this.fistSince == null) this.fistSince = t;
      const p = (t - this.fistSince) / c.fistHoldMs;
      if (!this.fistLatch) {
        this.progress = clamp01(p);
        this.label = "Minimizar…";
        if (p >= 1) {
          out.push({ kind: "window", action: "minimize" });
          this.fistLatch = true;
          this.label = "Minimizada";
        }
      }
    } else {
      this.fistSince = null;
      this.fistLatch = false;
    }

    return out;
  }

  private moveCursor(p: Pt, t: number, out: HaloAction[]) {
    const z = this.cfg.zone;
    const x = clamp01((p.x - z.x0) / (z.x1 - z.x0));
    const y = clamp01((p.y - z.y0) / (z.y1 - z.y0));
    const sx = this.fx.filter(x, t);
    const sy = this.fy.filter(y, t);
    this.cursor = { x: sx, y: sy };
    // Al empezar a pellizcar congelamos el cursor un instante para que el clic no se desvíe.
    if (t < this.freezeUntil) return;
    const e = this.emitted;
    if (!e || Math.hypot(e.x - sx, e.y - sy) > this.cfg.deadZone) {
      this.emitted = { x: sx, y: sy };
      out.push({ kind: "pointer", x: sx, y: sy });
    }
  }

  private endPinch(out: HaloAction[], allowClick: boolean) {
    if (this.pinch.phase === "pending" && allowClick) out.push({ kind: "button", action: "click" });
    if (this.pinch.phase === "drag") out.push({ kind: "button", action: "up" });
    this.pinch = { phase: "idle" };
  }

  /** true una sola vez cuando la palma lleva `stopHoldMs` quieta. */
  private holdStill(active: boolean, p: Pt, t: number): boolean {
    if (!active) {
      this.still = null;
      this.stopLatch = false;
      return false;
    }
    if (!this.still || Math.hypot(p.x - this.still.p.x, p.y - this.still.p.y) > this.cfg.stopStill) {
      this.still = { t0: t, p };
      this.stopLatch = false;
      return false;
    }
    if (this.stopLatch) return false;
    const prog = (t - this.still.t0) / this.cfg.stopHoldMs;
    this.progress = Math.max(this.progress, clamp01(prog));
    if (prog >= 1) {
      this.stopLatch = true;
      return true;
    }
    return false;
  }

  private lost(out: HaloAction[]) {
    this.endPinch(out, false);
    this.pinching = [false, false];
    this.bi = null;
    this.biLatch = false;
    this.fistSince = null;
    this.fistLatch = false;
    this.swipeTrail = [];
    this.still = null;
    this.stopLatch = false;
    this.cursor = null;
    this.fx.reset();
    this.fy.reset();
  }
}

const windowLabel: Record<Extract<HaloAction, { kind: "window" }>["action"], string> = {
  maximize: "Maximizar",
  restore: "Restaurar",
  minimize: "Minimizar",
  snap_left: "Mitad izquierda",
  snap_right: "Mitad derecha",
};
