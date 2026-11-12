# MicroPrompt: asistente de IA en llamadas (lo que aprendemos de Open-Cluely)

Estado: plan. Escrito el 11 de noviembre de 2026 tras estudiar
[Open-Cluely](https://github.com/shubhamshnd/Open-Cluely) (`96c0709`), una app
Electron que hace de copiloto en entrevistas y reuniones. Las tareas están en
Kanvas, grupo **Asistente [AS]**, y se suman a las de **Entrevistas [EN]**.

## Qué hace Open-Cluely

```ts
Open-Cluely (Electron, Windows primero)
├── Transcripción en vivo de 2 fuentes      AssemblyAI (nube, clave de pago)
│   ├── "Host" = audio del sistema (getUserMedia chromeMediaSource: desktop)
│   ├── "You"  = micrófono
│   └── junta los finales de cada fuente hasta 2,4 s de pausa en un mensaje
├── Hilo tipo chat                          renderer/features/ai-context/
│   ├── mensajes: voz (Host/You), capturas, respuestas de la IA, sistema
│   ├── cada mensaje tiene un interruptor AI / Off: entra o no en el contexto
│   └── el contexto se arma del más nuevo al más viejo hasta un tope de caracteres
├── Botones de IA (con atajo Alt+Shift+…, respuesta en streaming)
│   ├── Ask AI     todo: transcripción + capturas + respuestas anteriores
│   │              "Understanding" + respuesta completa; en código, solución + complejidad
│   ├── Suggest    sólo la transcripción: 2-4 frases para decir YA + 3 ideas + posibles repreguntas
│   ├── Screen AI  sólo las capturas: lee la pantalla (problema, error, diagrama) y responde
│   ├── Notes      5 apartados fijos: temas, decisiones, tareas, dudas, siguientes pasos
│   └── Insights   temas, puntos fuertes, carencias, recomendaciones
│   + email de seguimiento y pregunta suelta (sin botón en la UI)
├── Prompts                                 services/ai/prompts.js
│   ├── clasifica la pregunta: código, diseño de sistemas, conductual (STAR),
│   │   concepto, charla, otra; y responde con el formato de cada una
│   ├── corrige en silencio errores de transcripción ("link list" → "linked list")
│   └── lenguaje de programación preferido (ajuste)
├── Capturas de pantalla
│   ├── atajo: baja la opacidad de su ventana, captura la pantalla entera, la restaura
│   ├── máx. 50, se borran al limpiar; OCR con tesseract.js como apoyo
│   └── la imagen va al modelo (Gemini ve imágenes)
├── Modelos: Gemini (varias claves con relevo si se agota la cuota) u Ollama local
├── "Sigilo"
│   ├── setContentProtection: la ventana no sale al compartir pantalla
│   ├── siempre encima, sin barra de tareas, opacidad ajustable
│   ├── ocultar de golpe (y vaciar el chat), mover y redimensionar con atajos
│   └── se disfraza de Google Chrome (nombre, icono, editor "Google LLC")
└── Compañero móvil                         main-process/features/mobile-server/
    └── HTTP + WebSocket en 0.0.0.0:7823, sin contraseña: el móvil ve el hilo,
        pide capturas, pregunta a la IA y limpia
```

Lo que Open-Cluely **no** tiene: ayuda automática al acabar una pregunta (todo
es a botón), un guion propio con tus respuestas preparadas, ni nada local de
punta a punta (AssemblyAI siempre va a la nube).

## Qué ya tenemos en MicroPrompt

```ts
Open-Cluely                  →  MicroPrompt
├── AssemblyAI (nube)        →  Nemotron 3.5 en tu GPU (SU-01, CA-01)            hecho
├── Host / You               →  Los demás / Yo, con eco descartado (RE-01, RE-02) en revisión
├── ventana encima           →  ventana de subtítulos Rune (SU-02)                hecho
├── sesiones guardadas       →  historial .txt/.srt (SU-04)                       hecho
├── (no tiene)               →  guion de entrevista por secciones (EN-02)         en revisión
├── (no tiene)               →  tarjeta automática ante cada pregunta (EN-03)     bloqueada
└── Gemini / Ollama          →  Ollama directo; gemma4:12b recomendado (EN-01)    prototipo
```

## Qué falta, y cómo lo haríamos

Todo local. Ningún cambio necesita una clave ni sale del equipo:

- **Modelo:** sólo Ollama en `127.0.0.1:11434`, por HTTP de bucle local, como
  recomendó EN-01. `gemma4:12b` y `qwen3.5:9b`, que ya tienes, **ven
  imágenes**, así que la captura va directa al modelo y no hace falta OCR.
  Sin relevo de claves ni límites de cuota: no hay nube.
- **Ocultar al compartir:** las ventanas Rune ya saben excluirse de la captura
  (`exclude_from_capture`, lo usa Annotate). Así la captura de pantalla
  tampoco necesita el truco de bajar la opacidad.
- **Captura:** Annotate ya captura la pantalla en Windows
  (Windows.Graphics.Capture). Reutilizarlo para una imagen fija.
- **Prompts:** copiamos la idea de clasificar la pregunta y responder con un
  formato por tipo, en el idioma de la sesión, y metiendo tu guion, CV y
  oferta (EN-02) cuando estén cargados.

```ts
transcripción (Yo / Los demás) ─┐
captura de pantalla ────────────┼─► hilo de la sesión (cada mensaje: usar / no usar)
respuestas anteriores ──────────┘        │
                                         ▼  botón o atajo
                              prompt por acción + guion/CV/oferta
                                         │
                                         ▼
                              Ollama local (streaming) ─► respuesta en el hilo
```

Tareas (grupo AS), en orden:

1. **AS-01 Modelo local conectado** (base también para EN-03).
2. **AS-02 Hilo de la sesión** con el interruptor «usar en la IA».
3. **AS-03 Responder y Sugerir**, con atajos.
4. **AS-04 Preguntar por escrito** y repreguntar.
5. **AS-05 La IA ve tu pantalla.**
6. **AS-06 Notas al terminar** (Notas + Ideas), guardadas con la sesión.
7. **AS-07 Que no salga al compartir pantalla** (opcional, apagado por defecto).
8. **AS-08 El móvil como mando** (opcional; la última, si aún se quiere).

## Lo que no copiamos

- **El disfraz de Google Chrome** (nombre, icono y editor falsos): suplanta a
  otra empresa y no nos aporta nada.
- **El servidor móvil abierto sin contraseña en toda la red:** si se hace
  (AS-08), con un código de emparejamiento y la misma autenticación que la
  API local de Hex.
- **Email de seguimiento y pregunta suelta:** la pregunta escrita (AS-04) lo
  cubre; un email se pide escribiéndolo.
- **Mover y redimensionar con atajos, tamaños fijos:** la ventana ya se mueve
  con el ratón. Se añade si se echa en falta.
- **Lenguaje de programación preferido:** va como una línea en el prompt de
  AS-03, no como un ajuste, hasta que haga falta.

Ocultar la ventana al compartir pantalla es útil para tu propio guion, pero
úsalo sólo donde la ayuda de una IA esté permitida; Open-Cluely pide lo mismo.

## Riesgos

- **Memoria de la GPU (16 GB):** gemma4:12b ocupa 8,6 GB además de Nemotron;
  una imagen grande añade contexto. Medir en AS-05 y,
  si aprieta, usar qwen3.5:9b (6,3 GB).
- **Un solo stream en vivo:** RE-02 turna micrófono y sistema en uno. Lo que
  digas mientras habla el otro no entra en el hilo.
- **Tiempo de respuesta:** EN-01 midió 1,2 s hasta la tarjeta con 3 puntos. Una
  respuesta completa (Responder) con código tardará bastante más: el
  streaming hace que empiece a leerse en 1 o 2 s.
