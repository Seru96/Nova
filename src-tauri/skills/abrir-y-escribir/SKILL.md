---
name: abrir-y-escribir
description: Abrir una app y escribir o enviar algo en ella (Claude, WhatsApp, Telegram, Slack, Word, Pages…).
keywords: escribe, escribir, redacta, manda, envia, enviale, mensaje, whatsapp, telegram, slack, claude, word, pages
---
# Abrir una app y escribir en ella

La tarea NO termina al abrir la app. Haz todos los pasos seguidos, sin parar a preguntar:

1. Llama a `type_text` con `app` = nombre de la app y `text` = el texto exacto. `type_text` ya abre la app y la pone al frente, así que no hace falta llamar antes a `open_app`.
2. Si el usuario dice «envía», «manda» o «pregúntale», pon `submit: true` para pulsar Intro.
3. Si hay que abrir algo nuevo antes de escribir (una conversación nueva, un documento nuevo), usa `press_keys` con el atajo (Mac: `cmd+n`; Windows: `ctrl+n`) y `app`, y después `type_text`.
4. Termina con una frase: qué has escrito y dónde.

Ejemplo: «escribe en Claude "hazme un resumen de la reunión" y envíalo» →
`type_text {app: "Claude", text: "hazme un resumen de la reunión", submit: true}`.
