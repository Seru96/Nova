---
name: correo
description: Preparar un correo electrónico nuevo con destinatario, asunto y texto.
keywords: correo, email, e-mail, mail, gmail, outlook
---
# Preparar un correo

La forma más fiable es un enlace `mailto:` con `open_url`. Abre la app de correo con todo relleno.

1. Redacta tú el asunto y el cuerpo si el usuario solo da la idea.
2. Llama a `open_url` con `mailto:DESTINO?subject=ASUNTO&body=CUERPO`, codificando los espacios como `%20` y los saltos de línea como `%0A`. Si no hay destinatario, empieza por `mailto:?subject=...`.
3. No lo envíes tú: di que el correo está listo para revisar y enviar.

Ejemplo: «manda un correo a ana@empresa.com diciendo que llego tarde» →
`open_url {url: "mailto:ana@empresa.com?subject=Llego%20tarde&body=Hola%20Ana%2C%0Allego%20unos%20minutos%20tarde.%0AUn%20saludo"}`.
