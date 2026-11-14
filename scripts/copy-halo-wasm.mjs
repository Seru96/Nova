// Copia los ficheros wasm de MediaPipe a public/halo/wasm (no se versionan: pesan ~15 MB).
import { copyFileSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const dst = new URL("../public/halo/wasm/", import.meta.url);
mkdirSync(dst, { recursive: true });
for (const f of ["vision_wasm_module_internal.js", "vision_wasm_module_internal.wasm"]) {
  copyFileSync(require.resolve(`@mediapipe/tasks-vision/${f}`), new URL(f, dst));
}
console.log("H.A.L.O.: wasm de MediaPipe copiado a public/halo/wasm");
