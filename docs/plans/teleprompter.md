# MicroPrompt: teleprompter de subtítulos en vivo

Estado: plan. La Fase 1 (prototipo) está en
[`tools/microprompt-captions/`](../../tools/microprompt-captions/README.md),
probada sólo contra un Hex simulado. Escrito el 6 de noviembre de 2026 sobre
Hex `dev` en `49de3e16` (v3.3.0). Equipo de pruebas: Windows x64 con una
NVIDIA RTX 5070 Ti.

## Qué queremos

Mientras hablas, MicroPrompt muestra en pantalla lo que vas diciendo, como
subtítulos, con el menor retraso posible:

- Empieza y termina con un atajo o un botón. No hay que mantener una tecla
  pulsada, como en el dictado de Hex.
- Se ven 2 o 3 líneas grandes y legibles. La última línea crece palabra a
  palabra y las anteriores suben al completarse.
- Las palabras que el modelo ya no va a cambiar (estables) se distinguen de
  las provisionales.
- Todo ocurre en local, como en Hex. El audio no sale del equipo.

## Lo que Hex ya trae

Hex resuelve ya casi todo lo difícil. Sólo nos falta conectar las piezas de
otra forma.

```ts
Hex hoy
├── Motor de transcripción local          src/transcription/engine.rs
│   ├── un hilo que posee el modelo GGUF (transcribe.cpp) y ordena toda la inferencia
│   └── LiveStream / YieldingStream -> Partial { stable, tentative }
│       src/transcription/engine/streaming.rs
├── Modelos con streaming (streams: true)  src/transcription/models.rs
│   ├── Parakeet Unified English          sólo inglés
│   └── Nemotron 3.5 ASR                  27 idiomas, español incluido ("es")
│       el streaming sólo se activa en GPU (Metal o Vulkan); HEX_STREAMING=1 lo fuerza en CPU
├── Sesiones de transcripción continua    src/transcription/stream.rs
│   ├── corta el audio en ventanas en las pausas (máx. 25 s)  src/transcription/pause_segmenter.rs
│   ├── eventos Partial / Segment / Final
│   ├── cede el motor al dictado y acumula hasta 120 s de audio pendiente
│   └── transcribe offline la ventana completa si el modelo no hace streaming
├── API local /streams                    src/local_api/streams.rs, docs/streams.md
│   └── otra app empuja PCM de 16 kHz y recibe parciales por SSE
├── "Live text" (Labs)                    src/dictation/live_text.rs + src/rune_ui/live_text.rs
│   └── overlay de UNA línea junto al HUD, sólo mientras dictas (push-to-talk)
├── Captura de micrófono                  src/audio.rs (cpal, mono, 16 kHz tras remuestrear)
│   └── HEX_AUDIO_FIXTURE=<wav> sustituye el micro por un fichero (pruebas sin hablar)
└── Overlays Rune                         src/rune_hud.rs, src/rune_ui/live_text.rs
    └── ventanas sin foco, transparentes y que dejan pasar los clics, en macOS, Windows y Linux
```

Latencia medida por Hex con el streaming actual (`docs/features/dictation.md`):

- Las primeras palabras tardan de 2,2 a 5 s.
- Después el texto avanza cada 1,04 a 1,12 s (un "chunk" del modelo) y va
  1 o 2 s por detrás de la voz.
- Lo estable no se reescribe nunca. Con la configuración por defecto de
  Parakeet, lo tentativo llegó siempre vacío.
- El texto final coincidió palabra por palabra con la transcripción del clip
  completo en las pruebas en alemán y en inglés.

Este retraso de 1 o 2 s es nuestro punto de partida. Reducirlo es la Fase 4.

## Lo que falta

```ts
Teleprompter
├── Captura continua con inicio y parada (Hex sólo graba mientras mantienes el atajo
│   o durante una reunión)
├── Pasar ese audio a una sesión de src/transcription/stream.rs dentro del proceso
│   (hoy sólo la crea /streams, desde fuera, por HTTP)
├── Un "tablero" de subtítulos: segmentos completados + el parcial en curso,
│   recortado a las últimas N líneas
├── Un overlay de varias líneas, con letra grande, fondo semitransparente,
│   colocable y de tamaño ajustable
└── Controles: atajo, menú de bandeja y `hex teleprompter start|stop|toggle`
```

## Arquitectura propuesta

```ts
Micrófono (src/audio.rs, 16 kHz mono)
  │  bloques de 10 a 100 ms
  ▼
teleprompter::Session              src/teleprompter/session.rs   (nuevo)
  │  stream.push(samples) + latido (el lease de 10 s no caduca mientras hablas)
  ▼
transcription::stream::Stream      src/transcription/stream.rs   (reutilizado)
  │  ventanas cortadas en pausas -> YieldingStream del motor compartido
  │  Event::Partial { segment, stable, tentative } / Event::Segment / Event::Final
  ▼
teleprompter::board                src/teleprompter/board.rs     (nuevo; copia el patrón
  │  segmentos cerrados + parcial vivo,                            Board/Publisher/watch de
  │  recorte a N líneas                                            src/dictation/live_text.rs)
  ▼
rune_ui::teleprompter              src/rune_ui/teleprompter.rs   (nuevo; parte de
     overlay de 2 o 3 líneas: estable en color de texto,           src/rune_ui/live_text.rs)
     tentativo atenuado, sube al cerrarse cada segmento
```

Por qué reutilizar `transcription::stream` en vez de escribir otro bucle:

- Ya corta en pausas, mantiene acotadas la memoria y las ventanas, y vuelve a
  transcribir offline lo que el streaming no pudo.
- Ya convive con el dictado: si usas el atajo de Hex a la vez, el dictado va
  primero y los subtítulos se ponen al día después.
- Sus eventos (`partial`, `segment`) son justo lo que necesita un subtítulo.

Cambios pequeños que necesita:

- Poder crear un `Stream` desde dentro del proceso. `Streams::create(engine,
  format, selection, owner_token)` ya existe. Habrá que decidir si el
  teleprompter comparte el registro de `/streams` (máximo 1 stream) o tiene
  uno propio. Recomiendo compartirlo: sólo hay un motor y dos streams se
  pelearían por él.
- Una suscripción interna a los eventos, sin SSE. `Stream::subscribe` ya
  entrega los eventos en orden.
- Mantener vivo el lease con un latido desde el bucle de captura, o permitir
  crear un stream sin lease.

Para no complicar la sincronización con el Hex original, todo lo nuevo va en
módulos nuevos (`src/teleprompter/`, `src/rune_ui/teleprompter.rs`) y sólo se
tocan unos pocos puntos de enganche (registro de módulos, Labs, CLI,
bandeja). Así `git merge upstream/dev` sigue siendo viable.

## Equipo de pruebas: Windows x64 con RTX 5070 Ti

- **Hay parciales.** En Windows x64 Hex transcribe en la GPU con Vulkan 1.2,
  que viene con el driver de NVIDIA (`docs/windows.md`). Sólo ARM64 va por CPU.
- **Hex ya se midió en esta misma GPU**, con Parakeet Unified English
  (`docs/features/dictation.md`, 19 de octubre de 2026):
  - el texto final llegó 55 a 93 ms después de soltar (clips de 10 a 131 s),
    frente a 82 a 873 ms transcribiendo el clip completo;
  - la GPU estuvo ocupada sólo un 3 a 3,6 % del tiempo y el proceso usó
    unos 214 MB;
  - con chunks de 560 y 160 ms hubo más errores y hasta un 20 % de GPU, sin
    acelerar el final.

  Nemotron 3.5 en español no se ha medido en Windows: sólo en Linux con una
  Radeon 8060S, con un parcial cada 1,12 s.
- **Para la Fase 1 basta el Hex oficial**: `HexSetup.exe` x64. `/streams`
  está desde la 3.0.0 y Nemotron 3.5 ASR desde la 3.1.2. Ese Hex se
  actualiza solo desde el canal Dev.
- **Para compilar el fork** (Fases 0 y 2):
  - `scripts/setup-windows.ps1` instala lo que falte: Visual Studio C++ Build
    Tools, CMake y Rust con MSVC.
  - El SDK de Vulkan (`VULKAN_SDK`) es necesario para la GPU.
  - Después: `cargo run -- app`.
  - Una compilación así se identifica como `Method::Source` y no se actualiza
    sola. El instalador sí lo haría (`Method::Direct`).
- **Datos compartidos con el Hex instalado**:
  - `%APPDATA%\hex`: ajustes, historial y `local-api.json`.
  - `%LOCALAPPDATA%\hex`: logs y modelos.
  - Un evento con nombre que impide abrir dos copias.

  El fork tiene que renombrar todo esto (Fase 0) antes de convivir con Hex.
- **Riesgo en los overlays de Windows**: `docs/windows.md` (22 de octubre)
  dice que la app Rune "nunca se ha ejecutado en Windows". Las versiones
  3.0 a 3.3 traen arreglos de Windows, así que probablemente está
  desactualizado. Aun así, el HUD transparente ha fallado en algunos
  equipos: se veía negro (#499) o no se veía (#510), y sólo DX12 compone
  bien la transparencia. Por eso la Fase 1 muestra los subtítulos en una
  página web y el overlay nativo queda para la Fase 2.

## Fases

### Fase 0: preparar el fork (de 1 a 2 días)

Hecho en Windows el 7 de noviembre de 2026, salvo el renombrado. Se compila
con `scripts/setup-windows.ps1` (instaló CMake 4.4.4 y el SDK de Vulkan
1.4.363) y `cargo build --bin hex`, que tarda 3 min la primera vez; el motor
C++ se compila en Release también en debug. El fork convive con el Hex
instalado sin renombrar nada: con `HEX_APPLICATION_SUPPORT_DIR` usa su propia
carpeta de ajustes, logs y modelos, su propio `local-api.json` y su propio
evento de instancia única, y no se actualiza solo (`NotInstalled`).

```powershell
$env:HEX_APPLICATION_SUPPORT_DIR = "$env:APPDATA\microprompt"
.\target\debug\hex.exe app
py tools\microprompt-captions\captions.py --discovery "$env:APPDATA\microprompt\local-api.json" --open
```

Para no volver a descargar modelos, enlaza los del Hex instalado:
`New-Item -ItemType HardLink -Path "$env:APPDATA\microprompt\models\<fichero>" -Target "$env:APPDATA\hex\models\<fichero>"`.
Si los dos están abiertos, los dos responden al atajo de dictado.

El renombrado (nombre, identificadores, actualizador) queda para cuando haya
que empaquetar: tocar esas rutas ahora complicaría los `merge` con upstream.

- Compilar y ejecutar el fork tal cual en Windows (`docs/windows.md`:
  `scripts/setup-windows.ps1`, el SDK de Vulkan y `cargo run -- app`).
  Comprobar en **Settings › Speech engine** que usa la RTX 5070 Ti.
- Probar el dictado con **Nemotron 3.5 ASR** y el idioma en `es`, con
  Labs › Live text encendido. Es el primer contacto real con la latencia y la
  calidad de los subtítulos en español.
- Separar el fork del Hex instalado:
  - Nombre, identificadores y carpeta de datos (`src/app_paths.rs`): Hex usa
    `~/.local/share/hex`, `~/Library/Application Support/hex` y `%APPDATA%\hex`,
    además de un bloqueo de instancia única y `local-api.json`.
  - El actualizador (`src/updater.rs`) apunta a `anoma.ly/hex`. Una
    compilación desde el código (`Method::Source`) no se actualiza sola, pero
    un `.app` o un instalador empaquetado sí lo haría, y se reemplazaría por
    el Hex original. Hay que desactivarlo antes de empaquetar nada.
  - Los workflows de `.github/` (release, site, AUR, SDK) se disparan con
    push a `dev` o `production`, ramas que el fork no tiene. Hay que borrarlos
    o adaptarlos antes de crear esas ramas.
- Mantener `LICENSE` (MIT) y `THIRD_PARTY_NOTICES.md`.

### Fase 1: prototipo sin tocar Rust, con `/streams` (hecho y medido en Windows)

[`tools/microprompt-captions/`](../../tools/microprompt-captions/README.md),
en Python con numpy y sounddevice:

1. Lee `local-api.json` (puerto y token).
2. Hace `POST /streams` con `s16le` a 16 kHz.
3. Graba el micrófono (o reproduce un WAV en tiempo real), remuestrea a
   16 kHz y envía bloques de 100 ms a `POST /streams/{id}/audio`.
4. Escucha `GET /streams/{id}/events`.
5. Muestra los subtítulos en una página local configurable por URL (2.ª
   pantalla u OBS) y en la terminal.
6. Al terminar, guarda `.txt` y `.srt` y resume la latencia: cuándo llegan
   las primeras palabras, cada cuánto se actualizan los parciales y cuánto
   tarda el texto final de cada frase.

Comprobado sólo contra `mock_hex.py`, un Hex simulado que sigue
`docs/streams.md`:

- 12 tests automáticos;
- la página en Chromium, a mitad de una sesión, con la vista normal, la
  variante para OBS y el ancho de móvil.

Medido el 7 de noviembre de 2026 en Windows x64 con el Hex 3.3.0 oficial y
un WAV de 25,6 s en español (voz sintética Helena de Windows, 5 frases con
pausas de 1,5 a 2 s), resultados en el
[README del prototipo](../../tools/microprompt-captions/README.md#medido-en-windows):

- Nemotron 3.5 ASR en la RTX 5070 Ti (Vulkan): primeras palabras a 1,20 s,
  un parcial cada 1,12 a 1,14 s, cada frase cerrada 0,02 a 0,04 s tras su
  audio, transcripción exacta.
- Cohere Transcribe en la CPU: sin parciales, cada frase 0,52 s tras su audio.

- Micrófono real (2,5 min de habla libre, Nemotron en la RTX): 24 frases,
  casi todas exactas; se nota "bastante lento" (más de 1 s por detrás de la
  voz), que es lo que ataca la Fase 4.

Sin comprobar: ruido de fondo.

Límites: Hex tiene que estar abierto con el modelo cargado por el dictado (en
Linux y Windows, con el listener activo), sólo hay un stream a la vez y sólo
hay parciales con GPU y un modelo de streaming.

### Fase 2: modo Teleprompter nativo (de 1 a 2 semanas)

Siguiendo las convenciones del repo (`AGENTS.md`, `docs/features/labs.md`):

1. **Lab `teleprompter`**, apagado por defecto. Se añade a
   `labs::Feature`, con su fila en `src/rune_ui/labs.rs` y su clave en
   `hex settings`.
2. **`src/teleprompter/session.rs`**: abre el micrófono de forma continua
   (como hace la captura de reuniones), crea el `Stream`, empuja el audio y
   envía latidos. Se detiene al parar, si el modelo cambia o si se pierde el
   micrófono.
3. **`src/teleprompter/board.rs`**: guarda el estado del subtítulo
   (segmentos cerrados y parcial actual) y avisa a quien lo observe sin
   bloquear el hilo de audio, igual que `dictation/live_text.rs`.
4. **`src/rune_ui/teleprompter.rs`**: el overlay.
   - De 2 a 3 líneas con ajuste de línea. La más nueva abajo y las viejas
     suben con una animación corta.
   - Texto estable en `p.text()` y tentativo en `p.faint()`, como en
     `live_text.rs`. Soporte de derecha a izquierda heredado.
   - Tamaño de letra, número de líneas, opacidad del fondo y posición
     (abajo, arriba o libre) configurables.
   - Deja pasar los clics por defecto, con un modo "mover y redimensionar"
     que lo hace interactivo.
5. **Controles**: un atajo propio, una entrada en la bandeja o barra de menú
   y `hex teleprompter start|stop|toggle|status` en `control::cli`, que
   cualquier script o agente puede usar.
6. **Pruebas**: el board, con texto sintético. La sesión, con
   `HEX_AUDIO_FIXTURE` y un motor falso (`stream_tests.rs` ya trae uno). El
   overlay, con el renderizado offscreen y los goldens
   (`HEX_UPDATE_GOLDENS=1 cargo test --locked --bin hex rune_ui`, sólo en
   Linux).
7. **Documentación**: `docs/features/teleprompter.md` y una entrada en
   `docs/features/README.md`.

### Fase 3: pantalla y salida (de 3 a 5 días)

- Una ventana normal y opaca, además del overlay, para capturarla en OBS o
  mostrarla en otra pantalla.
- Exportar la sesión a `.srt` o `.vtt` con los tiempos de cada `Segment`
  (`startMs`/`endMs`). Hex ya exporta subtítulos de reuniones
  (`labs.subtitle_export`).
- Guardar la transcripción de cada sesión en el historial.

### Fase 4: menos latencia (investigación, continua)

En orden de coste:

1. **Mostrar ya lo tentativo**: el overlay lo pinta en cuanto llega.
2. **Chunks más cortos** (medido el 7 de noviembre de 2026):
   - `HEX_STREAM_CHUNK_MS` **no sirve para Nemotron**. Pide el streaming
     "buffered" de Parakeet Unified (`ParakeetBuffered`), y Nemotron, que es
     cache-aware, lo rechaza: `stream begin: invalid argument`. Hex se queda
     sin parciales.
   - Su control es `att_context_right` (`ParakeetStream`) ∈ {13, 6, 3, 0}, que
     equivale a 1040, 480, 240 y 0 ms de anticipación. Hex no lo expone y
     usa siempre 13.
   - Medido fuera de Hex con
     [`lookahead.py`](../../tools/microprompt-captions/lookahead.py): el
     mismo transcribe.cpp 0.4.1, el mismo GGUF Q8_0 y la RTX 5070 Ti, sobre
     el WAV en español de 25,6 s:

     | R | Parcial cada | Palabra en pantalla tras decirla (mediana / p90) | WER | GPU |
     | ---: | ---: | ---: | ---: | ---: |
     | 13 (Hex hoy) | 1120 ms | 493 / 1053 ms | 0 % | 7 % |
     | 6 | 560 ms | 256 / 493 ms | 0 % | 5 % |
     | **3** | **320 ms** | **172 / 413 ms** | **0 %** | **6 %** |
     | 0 | 160 ms | 410 / 570 ms | 2,2 % (pierde "Hola") | 12 % |

     R=3 recorta unos 320 ms en la mediana y 640 ms en el p90 sin perder
     precisión en este clip. R=0 es peor en todo. Es un único clip de voz
     sintética: hay que confirmarlo con voz real y más frases. El retraso
     extra de Hex (ventanas, SSE) va aparte.
   - **Hecho en el fork**: `HEX_STREAM_ATT_RIGHT` (en
     `src/transcription/gguf_session.rs`) pasa
     `StreamExtension::ParakeetStream { att_context_right }`; sin la
     variable, nada cambia. Medido con `captions.py` contra el fork, con el
     mismo WAV:

     | | Primeras palabras | Parcial cada | Frase final tras su audio | Texto |
     | --- | ---: | ---: | ---: | --- |
     | R=13 (por defecto) | 1,20 s | 1,16 s | 0,02 s | exacto |
     | R=3 | 0,90 s | 0,30–0,31 s | 0,02–0,04 s | exacto (2 pasadas) |

     La variable afecta a todo el streaming del proceso, también al dictado
     con el atajo. Con R=13 el texto final del streaming es idéntico al del
     clip completo; con R=3 puede variar.
   - **Por defecto desde CA-01**: sin variables, los streams de `/streams`
     (subtítulos) usan R=3 y el dictado con el atajo sigue en R=13
     (`OfflineGgufSession::captions_stream`). Un modelo que no acepta R
     (Parakeet Unified) hace los subtítulos como el dictado.
     `HEX_STREAM_ATT_RIGHT` o `HEX_STREAM_CHUNK_MS` siguen fijando un valor
     para todo. Medido con el WAV de Helena: subtítulos con parciales cada
     0,31 s y primeras palabras a 0,90 s; el dictado (`HEX_AUDIO_FIXTURE` y
     `hex dictation stop --print`) dio el mismo texto y los mismos 15
     parciales que antes del cambio.
   - Con voz real y micrófono (1 min, R=3) se percibió "bastante rápido" y
     sin pérdidas tras las pausas, con un error ("de espacio" por
     "despacio"). Primera sesión con R=13: "bastante lento". R=3 es el valor
     a usar para el teleprompter nativo (Fase 2).
3. **Ajustar las pausas** para subtítulos: pausa de 600 ms y trozo mínimo de
   3 s en `pause_segmenter.rs`. Cerrar líneas antes hace que suban antes.
4. **Sin GPU o con modelos sin streaming** (Parakeet v3, el más preciso en
   español): volver a decodificar los últimos 5 a 8 s cada 300 a 500 ms y
   fijar las palabras en las que coinciden dos pasadas seguidas (el "local
   agreement" de whisper_streaming). Parakeet v3 va unas 33 veces más rápido
   que el tiempo real en GPU, así que cabe en el presupuesto.
5. **macOS**: `SpeechTranscriber` de Apple da resultados "volátiles" al
   instante y gasta muy poca CPU (`docs/plans/meeting-live-transcription.md`).
   Requiere un puente Swift en streaming, que hoy no existe:
   `apple_speech.swift` sólo transcribe búferes completos.

### Distinguir interlocutores

Pregunta: ¿pueden los subtítulos decir quién habla? Sí, en local, y Hex ya
trae casi todo.

Lo que ya existe:

- **Reuniones**: Hex separa "You" (micrófono) de "Them" (audio del
  sistema) por la fuente, sin modelo.
- **Upstream** (sin fusionar en `dev`, 4 de noviembre de 2026):
  - estudio `docs/research/meeting-speakers.md` en la rama
    `speaker-diarization-spike`;
  - Lab "Speakers in meetings" en la rama `meeting-speakers-lab`
    (`src/meeting/speakers.rs`).

  Ambos diarizan "Them" **después** de la reunión con Sortformer v2.1 sobre
  `transcribe_cpp::DiarizeSession`, en trozos de 10 min que solapan 1 min.
- **transcribe.cpp 0.4.1** (la versión que fija Hex) ejecuta Sortformer:
  - hasta 4 voces;
  - presets de latencia de 1 a 30 s;
  - en GPU.

  Su API sólo diariza una grabación entera de una vez. El estado de
  streaming del modelo (caché de hablantes y FIFO) existe por dentro, pero
  no está expuesto.

Opciones, de menos a más trabajo:

```ts
Quién habla
├── A. Por fuente o canal: un micrófono por persona, o micrófono frente a audio del sistema
│   └── exacto y sin modelo; con /streams hace falta un stream por canal (hoy Hex admite 1) → Fase 2 nativa
├── B. Sortformer por lotes sobre el audio reciente (hecho en el prototipo: --speakers)
│   └── tools/microprompt-captions/speakers.py, con transcribe-cpp de PyPI, el mismo modelo
├── C. Sortformer en streaming de verdad: exponer SortformerStreamState en la API C de transcribe.cpp
│   └── etiqueta a ~1 s con el preset low_latency; trabajo en C++ en el fork de transcribe.cpp
└── D. Huellas de voz (CAM++ o ECAPA) para reconocer a alguien entre sesiones
    └── upstream: el vecino más cercano acierta el 81–94 % de las veces, pero ningún umbral fijo
        rechaza a un desconocido → sugerir nombres, no asignarlos solos
```

Lo que se aprendió al construir B (medido con voces reales, en CPU):

- Diarizar **toda la conversación de una vez** acertó todos los turnos (16 de
  16, con cualquier preset).
- Una **ventana que se desliza en cada pasada** confunde voces cuando corta a
  alguien a media frase y hay 4 voces dentro (entre 17 y 27 de 32 turnos con
  ventanas de 30 y 60 s).
- Pegar **muestras cortas de cada voz** delante de una ventana corta
  ("anclas") tampoco funcionó: entre 8 y 14 de 16.
- Lo que funciona son **bloques con inicio fijo**: el audio crece hasta 120 s
  y el bloque siguiente solapa 60 s. Las voces que no hablaron en el solape
  van como referencia al principio del bloque nuevo. Resultados en
  [`tools/microprompt-captions/README.md`](../../tools/microprompt-captions/README.md).

Para la Fase 2 nativa:

- Portar `SpeakerTracker` a Rust sobre `transcribe_cpp::DiarizeSession`,
  junto a `src/meeting/speakers.rs` de upstream si se fusiona.
- Usar el mismo artefacto: `SPEAKER_MODEL_ARTIFACT`, descargado sólo con el
  Lab activo.
- Si la latencia de etiqueta (al cerrarse cada frase) se queda corta,
  abordar la opción C en el fork de transcribe.cpp.

### Fase 5 (opcional): teleprompter con guion

Si además quieres leer un guion: cargas un texto, MicroPrompt alinea lo que
dices con el guion (coincidencia aproximada de palabras sobre los segmentos
estables), lo desplaza solo y marca por dónde vas. Usa la misma tubería de
las fases 1 a 4. Sólo cambian el board y la vista.

## Riesgos

- **Sin GPU no hay parciales.** Nemotron y Parakeet Unified sólo hacen
  streaming en Metal o Vulkan. Windows ARM64 es sólo CPU. Sin GPU, cada
  línea aparece entera al terminar la frase (tras la pausa), hasta que exista
  la Fase 4.4.
- **Calidad en español.** Hex recomienda Parakeet v3 para español, pero no
  hace streaming. Nemotron 3.5 sí lo hace, con más errores (3,05 % de WER en
  LibriSpeech frente a 1,95 % de Parakeet v3, y bastante peor en FLEURS). Hay
  que medirlo en español con tu voz y tu micrófono.
- **Palabras perdidas tras pausas largas** (#324, #338, #1101). El dictado lo
  mitiga con `PauseCap`. En `/streams` las ventanas se cortan en las pausas,
  así que debería afectar menos, pero hay que comprobarlo.
- **Tamaño del código**: unas 317.000 líneas de Rust y un upstream muy activo
  (más de 1.500 commits). Por eso conviene aislar los cambios en módulos
  nuevos y sincronizar con `git fetch upstream && git merge upstream/dev`.
- **Dependencias git**: Rune y el fork de transcribe.cpp vienen de
  `github.com/anomalyco/*`. Hoy son públicos. Si desaparecieran, habría que
  copiarlos a tu cuenta.

## Decisiones abiertas

Resueltas el 7 de noviembre de 2026; las tareas están en el tablero
(`kanvas.json`).

1. Sistema operativo y GPU: Windows x64 con una RTX 5070 Ti, sólo este PC,
   sin instalador ni renombrado.
2. Idioma: español e inglés, a elegir al empezar cada sesión.
3. Usos: leer tu propia voz en vivo, reuniones y llamadas, y teleprompter
   con guion (de un fichero `.txt` o `.md`; si improvisas, muestra lo que
   dices y retoma el guion al volver). Reuniones antes que el guion.
4. Dónde se ven: una ventana normal de la app, dentro del fork (Rust/Rune),
   con sólo texto. Se empieza y para con un botón en la ventana y con un
   atajo de teclado.
5. Fuentes de audio: micrófono, audio del sistema o ambos, a elegir por
   sesión. En reuniones basta "Yo / Los demás" por fuente, sin Sortformer.
6. Sesiones: se guardan en el historial de la app, exportables.
7. El fork sustituirá al Hex instalado, dictado incluido.
8. El prototipo de Python queda como banco de pruebas, sin funciones nuevas.
9. Pendiente: si se traen cambios de Hex (upstream) y cada cuánto.
