---
name: notas
description: Apuntar algo en una nota nueva (Notas en Mac, Bloc de notas en Windows) o en un archivo.
keywords: nota, notas, apunta, apuntame, anota, bloc de notas, lista de la compra
---
# Apuntar en una nota

En Mac (app Notas):
1. `press_keys {keys: "cmd+n", app: "Notas"}` para crear una nota nueva.
2. `type_text {text: "TÍTULO\nTEXTO"}` sin `app` (la nota ya está al frente).

En Windows (Bloc de notas):
1. `open_app {app: "Bloc de notas"}`.
2. `type_text {text: "TEXTO"}`.

Si el usuario prefiere guardarlo como archivo, usa `write_file` en ~/Documents/Nova/.
Confirma en una frase qué has apuntado.
