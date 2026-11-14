import { describe, expect, it } from "vitest";
import { GestureEngine, type Hand, type HaloAction } from "./gestures";

type Finger = "up" | "down";
interface Spec {
  cx: number;
  cy: number;
  s?: number;
  fingers?: [Finger, Finger, Finger, Finger];
  pinch?: boolean;
}

// Mano vertical en coordenadas "espejo" (las que ve el usuario); se devuelve sin espejo,
// como la entrega la cámara.
function hand({ cx, cy, s = 0.12, fingers = ["up", "up", "up", "up"], pinch = false }: Spec): Hand {
  const base: [number, number][] = Array(21).fill([0, 0]);
  base[0] = [0, 0.6];
  const cols = [-0.25, 0, 0.22, 0.42];
  const mcpY = [-0.4, -0.45, -0.4, -0.3];
  fingers.forEach((f, i) => {
    const mcp = 5 + i * 4;
    const x = cols[i];
    const y = mcpY[i];
    base[mcp] = [x, y];
    if (f === "up") {
      base[mcp + 1] = [x, y - 0.35];
      base[mcp + 2] = [x, y - 0.55];
      base[mcp + 3] = [x, y - 0.75];
    } else {
      base[mcp + 1] = [x, y - 0.3];
      base[mcp + 2] = [x, y - 0.1];
      base[mcp + 3] = [x, y + 0.15];
    }
  });
  base[1] = [-0.3, 0.4];
  base[2] = [-0.5, 0.2];
  base[3] = [-0.65, 0.05];
  base[4] = [-0.8, -0.1];
  if (pinch) {
    const [tx, ty] = base[8];
    base[4] = [tx - 0.04, ty + 0.03];
    base[3] = [tx - 0.2, ty + 0.25];
  }
  return base.map(([x, y]) => ({ x: 1 - (cx + x * s), y: cy + y * s }));
}

const point = (cx: number, cy: number) => hand({ cx, cy, fingers: ["up", "down", "down", "down"] });
const pinch = (cx: number, cy: number) => hand({ cx, cy, fingers: ["up", "down", "down", "down"], pinch: true });
const fist = (cx: number, cy: number) => hand({ cx, cy, fingers: ["down", "down", "down", "down"] });
const open = (cx: number, cy: number) => hand({ cx, cy });

function run(engine: GestureEngine, frames: [number, Hand[]][]): HaloAction[] {
  return frames.flatMap(([t, h]) => engine.update(h, t));
}
const kinds = (a: HaloAction[]) => a.filter((x) => x.kind !== "pointer");

const eng = () => new GestureEngine({ aspect: 1 });

describe("H.A.L.O. gestos", () => {
  it("el índice mueve el cursor en espejo", () => {
    const e = eng();
    const acts = run(e, [
      [0, [point(0.3, 0.4)]],
      [33, [point(0.31, 0.4)]],
      [400, [point(0.7, 0.4)]],
      [433, [point(0.7, 0.4)]],
      [800, [point(0.7, 0.4)]],
    ]);
    const ptrs = acts.filter((a) => a.kind === "pointer") as { x: number }[];
    expect(ptrs.length).toBeGreaterThan(1);
    expect(ptrs[ptrs.length - 1].x).toBeGreaterThan(ptrs[0].x);
    expect(e.hud.pose).toBe("point");
  });

  it("un pellizco corto es un clic", () => {
    const e = eng();
    const acts = run(e, [
      [0, [point(0.5, 0.4)]],
      [33, [pinch(0.5, 0.4)]],
      [100, [pinch(0.5, 0.4)]],
      [200, [point(0.5, 0.4)]],
    ]);
    expect(kinds(acts)).toEqual([{ kind: "button", action: "click" }]);
  });

  it("pellizcar y mover arrastra", () => {
    const e = eng();
    const frames: [number, Hand[]][] = [[0, [point(0.4, 0.4)]]];
    for (let i = 1; i <= 15; i++) frames.push([i * 33, [pinch(0.4 + i * 0.01, 0.4)]]);
    frames.push([600, [point(0.55, 0.4)]]);
    expect(kinds(run(e, frames))).toEqual([
      { kind: "button", action: "down" },
      { kind: "button", action: "up" },
    ]);
  });

  it("perder la mano mientras arrastra suelta el botón", () => {
    const e = eng();
    const acts = run(e, [
      [0, [pinch(0.4, 0.4)]],
      [500, [pinch(0.4, 0.4)]],
      [550, []],
      [900, []],
    ]);
    expect(kinds(acts)).toEqual([
      { kind: "button", action: "down" },
      { kind: "button", action: "up" },
    ]);
  });

  it("puño mantenido un segundo cierra la ventana una sola vez", () => {
    const e = eng();
    const frames: [number, Hand[]][] = [];
    for (let t = 0; t <= 2000; t += 50) frames.push([t, [fist(0.5, 0.5)]]);
    expect(kinds(run(e, frames))).toEqual([{ kind: "window", action: "close" }]);
  });

  it("deslizar la palma a la izquierda va al escritorio de la derecha", () => {
    const e = eng();
    const acts = run(e, [
      [0, [open(0.7, 0.5)]],
      [60, [open(0.6, 0.5)]],
      [120, [open(0.5, 0.5)]],
      [180, [open(0.4, 0.5)]],
      [240, [open(0.3, 0.5)]],
    ]);
    expect(kinds(acts)).toEqual([{ kind: "desktop", dir: "right" }]);
  });

  it("dos manos separándose maximizan", () => {
    const e = eng();
    const acts = run(e, [
      [0, [pinch(0.4, 0.5), pinch(0.55, 0.5)]],
      [100, [pinch(0.35, 0.5), pinch(0.6, 0.5)]],
      [200, [pinch(0.25, 0.5), pinch(0.7, 0.5)]],
      [300, [pinch(0.2, 0.5), pinch(0.75, 0.5)]],
      [400, [point(0.2, 0.5), point(0.75, 0.5)]],
    ]);
    expect(kinds(acts)).toEqual([{ kind: "window", action: "maximize" }]);
  });

  it("dos manos desplazándose a la derecha colocan la ventana a la derecha", () => {
    const e = eng();
    const acts = run(e, [
      [0, [pinch(0.3, 0.5), pinch(0.5, 0.5)]],
      [150, [pinch(0.4, 0.5), pinch(0.6, 0.5)]],
      [300, [pinch(0.5, 0.5), pinch(0.7, 0.5)]],
    ]);
    expect(kinds(acts)).toEqual([{ kind: "window", action: "snap_right" }]);
  });

  it("palma quieta pausa y vuelve a reanudar", () => {
    const e = eng();
    const frames: [number, Hand[]][] = [];
    for (let t = 0; t <= 1300; t += 50) frames.push([t, [open(0.5, 0.5)]]);
    expect(kinds(run(e, frames))).toEqual([{ kind: "stop" }]);
    expect(e.hud.paused).toBe(true);
    // En pausa los pellizcos no hacen nada.
    expect(kinds(run(e, [[1400, [pinch(0.5, 0.5)]], [1450, [point(0.5, 0.5)]]]))).toEqual([]);
    const again: [number, Hand[]][] = [];
    for (let t = 1500; t <= 2800; t += 50) again.push([t, [open(0.5, 0.5)]]);
    expect(kinds(run(e, again))).toEqual([{ kind: "resume" }]);
    expect(e.hud.paused).toBe(false);
  });
});
