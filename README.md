# Nova

Asistente de escritorio al estilo J.A.R.V.I.S., *local-first*, para macOS y Windows.
Tauri v2 (Rust) + React 19 + TypeScript + Tailwind + SQLite.

Nova responde a **«Oye Nova»**, piensa con un modelo local que viene integrado en la app y
solo recurre a Claude (con tu propia API key) cuando la tarea lo merece. Con el **Modo
Confidencial** activo, el backend bloquea cualquier conexión con Claude.

## Qué hace la fase 1

| Pieza | Cómo |
| --- | --- |
| Modelo local | `llama-server` (llama.cpp) gestionado por Nova. En el primer arranque descarga el runtime (Metal en Mac, Vulkan en Windows) y **Qwen3 4B Instruct 2507** (Q4_K_M, ~2,5 GB). No hace falta instalar Ollama ni LM Studio. |
| Enrutador | Local por defecto. El modelo local tiene la herramienta `escalate_to_claude`; también puedes pedirlo («pregúntale a Claude…»). |
| Modo Confidencial | Switch en la cabecera o por voz/texto («esto es confidencial», «modo privado», «desactiva el modo privado»). Se comprueba en Rust justo antes de abrir la conexión y durante el streaming. |
| Identidad | Un único prompt de sistema (`src-tauri/src/persona.rs`) para el modelo local y para Claude: siempre es Nova. |
| Voz | Switch de escucha. Micrófono → VAD por energía → whisper.cpp (`ggml-small-q5_1`, offline) → wake word «Oye Nova» → orden. Botón 🎙 para hablar sin decir la wake word. |
| Habla | Voces del sistema en español (Mónica/Paulina en Mac, Helena/Laura en Windows). Gratis, offline. Se silencia el micrófono mientras habla. |
| Ajustes | Nombre, personalidad (por defecto con humor) + instrucciones propias, API key en el llavero del sistema, modelo de Claude, voz. |
| Control de apps | Herramientas `open_app`, `type_text`, `press_keys` y `open_url` para el modelo local y para Claude; encadena hasta 12 pasos («abre Notas y escribe…», «escribe en Claude…»). En Mac pide el permiso de Accesibilidad. Se puede desactivar en Ajustes. |
| Archivos | `list_dir`, `find_files`, `search_text`, `read_file`, `write_file`, `edit_file` y `open_path`, para el modelo local y para Claude. Solo dentro de la carpeta del usuario, nunca carpetas de credenciales y sin borrar nada (`files.rs`). |
| Skills | Recetas en Markdown con el formato de las skills de Claude (`<config>/skills/<nombre>/SKILL.md`). Vienen de serie: abrir-y-escribir, correo, buscar-en-internet, musica-y-videos, archivos y notas. La que encaja con la petición se mete en el prompt (el modelo local es pequeño) y además hay `use_skill`. |
| Interfaz | Mismo lenguaje visual que RackMap: barra lateral con conversaciones (buscar, borrar), tema Oscuro, Híbrido (paneles oscuros y conversación en blanco) o Claro, como RackMap. |
| H.A.L.O. | Control de **todo el ordenador** con gestos de la mano por la cámara (switch «Gestos»). MediaPipe Hand Landmarker (21 puntos por mano) en un Web Worker dentro de un HUD transparente que deja pasar los clics. Índice = cursor, pellizco = clic/arrastrar, pulgar+corazón = clic derecho, dos manos = maximizar/restaurar/media pantalla, deslizar la palma = cambiar de escritorio, puño 1 s = cerrar la ventana activa, palma quieta = alto (pausa escucha y gestos). Ventanas con Accessibility en Mac y Win32 en Windows (`window_manager.rs`). |
| Persistencia | Conversaciones en SQLite (`tauri-plugin-sql`); ajustes en `settings.json` del directorio de configuración. |

## Requisitos para compilar

- Rust estable, Node 20+ y `pnpm`.
- CMake (whisper.cpp se compila dentro de la app).
- **macOS**: Xcode Command Line Tools (`xcode-select --install`).
- **Windows**: Visual Studio Build Tools (C++), CMake y LLVM (para `libclang`, que usa whisper-rs).

## Arrancar

```bash
pnpm install
pnpm tauri dev
```

La primera vez Nova descarga ~2,7 GB (motor + modelos) a la carpeta de datos de la app y
muestra el progreso en la ventana. Las siguientes veces arranca directamente.

Para crear el instalador: `pnpm tauri build`.

### Variables útiles en desarrollo

- `NOVA_LLAMA_SERVER=/ruta/a/llama-server`: usa un `llama-server` propio en vez de descargarlo.
- `NOVA_ANTHROPIC_API_KEY=sk-ant-…`: API key sin pasar por el llavero.

## Estructura

```
src/                      Frontend React
  App.tsx                 Estado, eventos del backend y persistencia
  components/             Cabecera, chat, compositor, ajustes
  lib/api.ts              Comandos y eventos tipados de Tauri
  lib/db.ts               SQLite
  halo/                   HUD, worker de visión y máquina de estados de gestos (con tests)
src-tauri/src/
  llm/local.rs            Descarga y gestión de llama-server + chat en streaming
  halo.rs                 H.A.L.O.: HUD, ratón y acciones de los gestos
  window_manager.rs       Maximizar/colocar/minimizar la ventana activa (Mac y Windows)
  control.rs              Abrir apps, escribir y pulsar atajos (enigo)
  llm/claude.rs           Cliente de Claude con el bloqueo confidencial
  router.rs               Enrutador local → Claude
  persona.rs              Identidad y personalidad comunes
  intents.rs              Wake word y órdenes de privacidad (deterministas)
  voice/                  Micrófono + VAD, whisper, voces del sistema
  settings.rs, secrets.rs Ajustes y llavero
```

## Hoja de ruta

1. **Básico funcional** (esta fase).
2. Funciones: H.A.L.O. (hecho), memoria compacta a largo plazo, skills.
3. UI/UX: HUD transparente estilo *liquid glass*, atajos globales.
4. Consumo y batería: wake word dedicada más ligera, CUDA en Windows, ajustes finos.
