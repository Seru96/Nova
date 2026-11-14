---
name: buscar-en-internet
description: Buscar algo en internet, abrir una web o ver noticias en el navegador.
keywords: internet, google, web, navegador, safari, chrome, noticias, pagina, busca en, buscame
---
# Buscar en internet o abrir una web

No tienes acceso directo a internet, pero puedes abrir el navegador:

- Búsqueda: `open_url` con `https://www.google.com/search?q=TÉRMINOS` (espacios como `+`).
- Una web concreta: `open_url` con su dirección completa (`https://…`).
- Mapas: `https://www.google.com/maps/search/LUGAR`.
- Traducir: `https://translate.google.com/?sl=auto&tl=es&text=TEXTO`.

Después di en una frase qué has abierto. Si la pregunta necesita un razonamiento largo y no hay Modo Confidencial, puedes usar `escalate_to_claude`.
