---
name: archivos
description: Encontrar, leer, resumir, crear o editar archivos y carpetas del ordenador.
keywords: archivo, archivos, fichero, carpeta, documento, documentos, escritorio, descargas, pdf, txt, lee, leer, resume, guarda, crea un archivo, busca el archivo
---
# Trabajar con archivos

Herramientas: `list_dir`, `find_files`, `search_text`, `read_file`, `write_file`, `edit_file`, `open_path`.
Rutas: `~` es la carpeta del usuario; vale «~/Escritorio», «~/Documentos», «~/Descargas».

- «¿Qué hay en el escritorio?» → `list_dir {path: "~/Escritorio"}` y resume lo importante.
- «Busca el archivo X» → `find_files {name: "X"}` (o `"*.pdf"`). Si hay varios, enumera los más probables.
- «¿Dónde puse lo de…?» → `search_text {query: "…", path: "~/Documents"}`.
- «Lee / resume este archivo» → `read_file` y luego responde. Un PDF o una imagen no se pueden leer como texto: ábrelos con `open_path`.
- «Guarda esto en un archivo» → `write_file` con una ruta clara (por defecto en ~/Documents/Nova/).
- «Cambia X por Y en el archivo» → `read_file` primero y después `edit_file` con el fragmento exacto.

Nunca borres nada. Cuando acabes, di el nombre y la ruta del archivo.
