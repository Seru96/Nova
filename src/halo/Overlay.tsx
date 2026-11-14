// HUD transparente de H.A.L.O.: cámara, detección y respuesta visual discreta.
// Esta ventana cubre la pantalla, siempre encima y sin recibir clics (una vez lista).
import { useEffect, useRef, useState } from "react";
import { api, on } from "../lib/api";
import { GestureEngine, type Hand, type HaloAction, type HudState } from "./gestures";
import type { WorkerOut } from "./vision.worker";

const BONES: [number, number][] = [
  [0, 1], [1, 2], [2, 3], [3, 4],
  [0, 5], [5, 6], [6, 7], [7, 8],
  [5, 9], [9, 10], [10, 11], [11, 12],
  [9, 13], [13, 14], [14, 15], [15, 16],
  [13, 17], [0, 17], [17, 18], [18, 19], [19, 20],
];

const FRAME_MS = 33;

function dispatch(a: HaloAction) {
  const fail = (e: unknown) => console.warn("H.A.L.O.", e);
  switch (a.kind) {
    case "pointer":
      void api.haloPointer(a.x, a.y).catch(fail);
      break;
    case "button":
      void api.haloButton(a.action).catch(fail);
      break;
    case "window":
      void api.haloWindow(a.action).catch(fail);
      break;
    case "desktop":
      void api.haloDesktop(a.dir).catch(fail);
      break;
    case "stop":
      void api.haloStopListening().catch(fail);
      break;
    case "resume":
      break;
  }
}

type Status = { phase: "starting" } | { phase: "running"; delegate: string } | { phase: "error"; message: string };

export function Overlay() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [hud, setHud] = useState<HudState | null>(null);
  const [status, setStatus] = useState<Status>({ phase: "starting" });
  const [preview, setPreview] = useState(true);
  const [noAccess, setNoAccess] = useState(false);
  const handsRef = useRef<Hand[]>([]);

  useEffect(() => {
    document.documentElement.classList.add("halo-overlay");
    api.getSettings().then((s) => setPreview(s.halo_preview)).catch(() => {});
    const unsub = on("settings://changed", (s) => setPreview(s.halo_preview));
    const unsubAccess = on("halo://access", (a) => setNoAccess(!a.ok));

    let alive = true;
    let stream: MediaStream | null = null;
    let busy = false;
    let lastSent = 0;
    let frameHandle = 0;
    let running = false;
    const engine = new GestureEngine({ aspect: 640 / 480 });
    const worker = new Worker(new URL("./vision.worker.ts", import.meta.url), { type: "module" });

    const fail = (message: string) => {
      if (!alive) return;
      setStatus({ phase: "error", message });
      void api.haloReady(false);
    };

    const loop = () => {
      if (!alive) return;
      const video = videoRef.current;
      const now = performance.now();
      if (video && video.readyState >= 2 && !busy && now - lastSent >= FRAME_MS) {
        busy = true;
        lastSent = now;
        createImageBitmap(video)
          .then((bitmap) => worker.postMessage({ type: "frame", bitmap, ts: now }, [bitmap]))
          .catch(() => (busy = false));
      }
      frameHandle = requestAnimationFrame(loop);
    };

    worker.onmessage = (e: MessageEvent<WorkerOut>) => {
      const m = e.data;
      if (m.type === "ready") {
        running = true;
        setStatus({ phase: "running", delegate: m.delegate });
        void api.haloReady(true);
        loop();
      } else if (m.type === "error") {
        busy = false;
        if (!running) fail(`No he podido cargar el detector de manos: ${m.message}`);
      } else if (m.type === "hands") {
        busy = false;
        handsRef.current = m.hands;
        for (const a of engine.update(m.hands, m.ts)) dispatch(a);
        setHud(engine.hud);
      }
    };

    (async () => {
      try {
        stream = await navigator.mediaDevices.getUserMedia({
          video: { width: 640, height: 480, facingMode: "user", frameRate: { ideal: 30 } },
          audio: false,
        });
      } catch (e) {
        fail(`No tengo acceso a la cámara (${e instanceof Error ? e.name : e}). Revisa el permiso de Cámara de Nova en los ajustes del sistema.`);
        return;
      }
      if (!alive) return stream.getTracks().forEach((t) => t.stop());
      const video = videoRef.current!;
      video.srcObject = stream;
      await video.play().catch(() => {});
      worker.postMessage({ type: "init", base: new URL("/halo", location.href).href });
    })();

    return () => {
      alive = false;
      cancelAnimationFrame(frameHandle);
      worker.terminate();
      stream?.getTracks().forEach((t) => t.stop());
      void unsub.then((f) => f());
      void unsubAccess.then((f) => f());
    };
  }, []);

  // Miniatura en espejo con el esqueleto de la mano.
  useEffect(() => {
    if (!preview) return;
    let raf = 0;
    const draw = () => {
      const c = canvasRef.current;
      const v = videoRef.current;
      const ctx = c?.getContext("2d");
      if (c && v && ctx && v.readyState >= 2) {
        const w = c.width;
        const h = c.height;
        ctx.save();
        ctx.translate(w, 0);
        ctx.scale(-1, 1);
        ctx.globalAlpha = 0.55;
        ctx.drawImage(v, 0, 0, w, h);
        ctx.globalAlpha = 1;
        ctx.lineWidth = 2;
        ctx.strokeStyle = "rgba(10,132,255,0.95)";
        ctx.fillStyle = "#fff";
        for (const hand of handsRef.current) {
          ctx.beginPath();
          for (const [a, b] of BONES) {
            ctx.moveTo(hand[a].x * w, hand[a].y * h);
            ctx.lineTo(hand[b].x * w, hand[b].y * h);
          }
          ctx.stroke();
          for (const p of hand) {
            ctx.beginPath();
            ctx.arc(p.x * w, p.y * h, 2, 0, Math.PI * 2);
            ctx.fill();
          }
        }
        ctx.restore();
      }
      raf = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, [preview]);

  const cursor = hud?.cursor;
  const label = noAccess
    ? "Falta el permiso de Accesibilidad: actívalo para Nova en Ajustes del Sistema y reinicia Nova"
    : hud?.paused
      ? "H.A.L.O. en pausa · palma quieta para seguir"
      : hud?.label;

  return (
    <div className="halo">
      <video ref={videoRef} muted playsInline className="halo-video" />

      {cursor && !hud?.paused && (
        <div
          className={`halo-cursor ${hud?.pose === "pinch" ? "pinch" : ""} ${hud?.dragging ? "drag" : ""}`}
          style={{ left: `${cursor.x * 100}%`, top: `${cursor.y * 100}%` }}
        />
      )}

      {status.phase === "error" ? (
        <div className="halo-card">
          <strong>H.A.L.O.</strong>
          <p>{status.message}</p>
          <button className="btn primary" onClick={() => void api.haloSet(false)}>
            Desactivar gestos
          </button>
        </div>
      ) : (
        <div className="halo-dock">
          {preview && (
            <div className="halo-preview">
              <canvas ref={canvasRef} width={192} height={144} />
            </div>
          )}
          <div className={`halo-pill ${label ? "show" : ""} ${hud?.paused || noAccess ? "paused" : ""}`}>
            <span className={`halo-dot ${status.phase === "running" && (hud?.hands ?? 0) > 0 ? "live" : ""}`} />
            <span>{status.phase === "starting" ? "Encendiendo la cámara…" : label || "H.A.L.O."}</span>
            {(hud?.progress ?? 0) > 0 && (
              <span className="halo-progress">
                <span style={{ width: `${(hud?.progress ?? 0) * 100}%` }} />
              </span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
