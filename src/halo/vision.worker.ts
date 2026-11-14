// H.A.L.O.: detección de manos (MediaPipe Hand Landmarker) fuera del hilo principal.
import { HandLandmarker } from "@mediapipe/tasks-vision";

type In =
  | { type: "init"; base: string }
  | { type: "frame"; bitmap: ImageBitmap; ts: number };

export type WorkerOut =
  | { type: "ready"; delegate: "GPU" | "CPU" }
  | { type: "error"; message: string }
  | { type: "hands"; ts: number; hands: { x: number; y: number; z: number }[][] };

let detector: HandLandmarker | null = null;
const post = (m: WorkerOut) => (self as unknown as Worker).postMessage(m);

async function create(base: string, delegate: "GPU" | "CPU") {
  return HandLandmarker.createFromOptions(
    {
      wasmLoaderPath: `${base}/wasm/vision_wasm_module_internal.js`,
      wasmBinaryPath: `${base}/wasm/vision_wasm_module_internal.wasm`,
    },
    {
      baseOptions: { modelAssetPath: `${base}/hand_landmarker.task`, delegate },
      runningMode: "VIDEO",
      numHands: 2,
      minHandDetectionConfidence: 0.6,
      minHandPresenceConfidence: 0.5,
      minTrackingConfidence: 0.5,
    },
  );
}

self.onmessage = async (e: MessageEvent<In>) => {
  const m = e.data;
  if (m.type === "init") {
    try {
      detector = await create(m.base, "GPU");
      post({ type: "ready", delegate: "GPU" });
    } catch {
      try {
        detector = await create(m.base, "CPU");
        post({ type: "ready", delegate: "CPU" });
      } catch (err) {
        post({ type: "error", message: String(err) });
      }
    }
    return;
  }
  if (m.type === "frame") {
    if (!detector) {
      m.bitmap.close();
      return;
    }
    try {
      const r = detector.detectForVideo(m.bitmap, m.ts);
      post({ type: "hands", ts: m.ts, hands: r.landmarks.map((h) => h.map(({ x, y, z }) => ({ x, y, z }))) });
    } catch (err) {
      post({ type: "error", message: String(err) });
    } finally {
      m.bitmap.close();
    }
  }
};
