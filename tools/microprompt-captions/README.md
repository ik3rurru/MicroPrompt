# MicroPrompt captions (prototipo, Fase 1)

Subtítulos en vivo de lo que dices, usando la API `/streams` del Hex que ya
tienes abierto ([`docs/streams.md`](../../docs/streams.md)). No modifica Hex
ni hace falta compilarlo: sirve para medir la latencia real y probar la
experiencia antes de construir el modo nativo
([plan](../../docs/plans/teleprompter.md)).

```ts
micrófono ─► captions.py ─► Hex /streams (Nemotron 3.5 en tu GPU)
                 │
                 ├─► página http://127.0.0.1:8765/  (navegador, 2.ª pantalla, OBS)
                 ├─► terminal (--console)
                 └─► al terminar: transcripción (--txt), subtítulos (--srt) y latencias
```

## Puesta en marcha en Windows (RTX 5070 Ti)

### 1. Hex

1. Instala Hex para Windows x64 desde
   [`HexSetup.exe`](https://anoma.ly/hex/download/windows-x64-setup). Para esta
   fase basta el Hex oficial: la versión 3.3.0 ya incluye `/streams`. Ten en
   cuenta que se actualiza solo y guarda sus datos en `%APPDATA%\hex`. La
   compilación del fork vendrá en la Fase 0 del plan.
2. Actualiza el driver de NVIDIA. Hex usa la GPU a través de Vulkan 1.2, que
   viene con el driver. En **Settings › Speech engine** debe aparecer la GPU,
   no la CPU.
3. Elige el modelo **Nemotron 3.5 ASR** con el idioma **Español**. Es el único
   modelo que hace streaming en español; Hex lo descarga la primera vez.
4. Deja Hex **Enabled**: así el motor carga el modelo y `/streams` puede
   usarlo.
5. Opcional: activa **Labs › Live text** y haz un dictado con `Ctrl+Space`.
   Verás las palabras en directo junto al HUD, una buena primera prueba.

Para confirmar que hay streaming, busca en `%LOCALAPPDATA%\hex\logs\process.log`
la línea `streaming transcription is available`.

### 2. Python

```powershell
winget install Python.Python.3.12
py -m pip install numpy sounddevice
```

### 3. Ejecutar

Desde esta carpeta:

```powershell
py captions.py --list-devices            # ver micrófonos
py captions.py --open --console          # micrófono por defecto, abre la página
py captions.py --device "Yeti" --language es --srt sesion.srt --txt sesion.txt
```

Habla y para con `Ctrl+C`. El script pide a Hex que termine lo pendiente y
después imprime la transcripción y un resumen de latencia:

```text
— Latencia —
  primeras palabras: a los 2.40 s de audio enviado
  actualizaciones parciales: 57, cada 1.12 s (mediana)
  texto final de cada frase: 0.85 s tras acabar su audio (mediana; p90 1.30 s; 9 frases)
```

(Valores de ejemplo, no medidos.) `--language es` comprueba que Hex tiene
cargado el español; si no, el script avisa en lugar de transcribir con otro
idioma.

## La página de subtítulos

`http://127.0.0.1:8765/` muestra sólo texto: toda la sesión, que sube según
hablas. Puedes hacer scroll hacia arriba para releer; al volver abajo sigue
el texto nuevo. Admite opciones en la URL:

| Opción | Qué hace | Por defecto |
| --- | --- | --- |
| `size` | tamaño de letra en px | `56` |
| `lines` | número fijo de líneas visibles, las anteriores salen por arriba (para OBS) | toda la sesión |
| `align` | `center` o `left` | `center` |
| `position` | `bottom`, `middle` o `top` | `bottom` |
| `width` | ancho del bloque, en % de la ventana | `90` |
| `fg`, `dim`, `bg` | colores del texto, de las palabras provisionales y del fondo (`#` se escribe `%23`) | blanco, gris, negro |
| `font`, `weight` | familia y grosor de letra | fuente del sistema, `600` |
| `transparent=1` | fondo transparente con contorno, para OBS | |
| `status=1` | muestra la línea de estado (conexión, modelo, errores) | oculta |

Ejemplos:

- **OBS**: añade una fuente *Navegador* (Browser Source) con
  `http://127.0.0.1:8765/?transparent=1&size=64&lines=2` y su ancho y
  alto de lienzo.
- **Segunda pantalla**: abre la página y pulsa `F11`.
- **Ventana pequeña siempre encima**: abre una ventana sin barras con
  `Start-Process msedge '--app=http://127.0.0.1:8765/?size=40&lines=2'` y
  fíjala encima con PowerToys › *Always On Top* (`Win+Ctrl+T`). El overlay
  nativo, que deja pasar los clics, llegará en la Fase 2.

Las palabras ya fijadas salen en blanco, las provisionales en gris, y las
frases terminadas se atenúan un poco y suben.

## Medido en Windows

7 de noviembre de 2026, Hex 3.3.0 oficial, Windows x64 (Ryzen 7 9800X3D,
RTX 5070 Ti). Audio: un WAV de 25,6 s en español generado con la voz Helena
de Windows (SAPI), 5 frases con pausas de 1,5 a 2 s, enviado con `--wav` a
velocidad real.

| Modelo | Dónde | Parciales | Primeras palabras | Parcial cada | Frase final tras su audio (mediana / p90) |
| --- | --- | --- | ---: | ---: | ---: |
| Nemotron 3.5 ASR | RTX 5070 Ti (Vulkan) | sí, 14 | 1,20 s | 1,12–1,14 s | 0,02–0,04 s / 0,14 s |
| Cohere Transcribe | CPU | no | — | — | 0,52 s / 0,94 s |

Las dos pasadas con Nemotron y la de Cohere dieron la transcripción exacta
(Cohere escribió "Micro Prompt").

Con micrófono (auriculares Astro A50, voz real, habla libre de 2,5 min con
Nemotron en la RTX): 24 frases, casi todas exactas. Los errores fueron una
conjugación ("si habla" por "si hablo"), una frase partida en dos por una
pausa a media frase ("Este modelo" / "Podría ser mejor…") y una frase
fantasma de sólo "¿" en un silencio. Sensación al usarlo: correcto, pero
"bastante lento"; el texto va algo más de un segundo por detrás de la voz.
Sólo se oye el micrófono, no el audio de Windows.

Con el fork y `HEX_STREAM_ATT_RIGHT=3` (mismo micrófono, 1 min de habla
libre): 13 frases. La sensación cambió a "bastante rápido", "es una mejora",
"aceptable", y no se perdió nada tras las pausas. Un error: "despacio" salió
"de espacio", y bien al repetirlo. A algunas frases les faltó el punto final,
como ya pasaba con el valor por defecto.

Ojo: si la GPU falla una vez (Hex guardó un "out of memory" con Cohere en la
RTX), Hex la apunta en `%APPDATA%\hex\gpu-failures.json` y en modo Auto pasa
a la CPU, sin parciales. `hex settings set transcription_device gpu` (o
Settings › Speech engine › GPU) la vuelve a probar.

## Qué medir en esta fase

1. Leer un párrafo en voz alta a ritmo normal (`--srt`, para revisar la
   calidad después).
2. Hablar libremente, con pausas de 2 a 10 s.
3. La misma prueba con ruido de fondo (música baja, ventilador).
4. Para comparar: en inglés con **Parakeet Unified English**.

Anota el resumen de latencia de cada sesión y los errores de transcripción
más frecuentes en español. Con esos datos se decide la Fase 4 del plan
(chunks más cortos, ajuste de pausas o el modo "local agreement").

Chunks más cortos: `HEX_STREAM_CHUNK_MS` sólo afecta a Parakeet Unified
English. Con Nemotron, Hex rechaza la opción al empezar el stream y se queda
**sin parciales**, así que no la pongas. El ajuste de Nemotron es la
anticipación (`att_context_right`), que Hex no expone. `lookahead.py` la mide
fuera de Hex, con el mismo motor y el mismo modelo:

```powershell
py -m pip install transcribe-cpp
py lookahead.py "$env:APPDATA\hex\models\nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf" prueba.wav "texto que se dice"
```

En la RTX 5070 Ti, R=3 (un parcial cada 320 ms) mostró cada palabra a unos
170 ms de decirla (mediana), frente a unos 490 ms con R=13, el valor que usa
Hex, sin perder precisión en un clip de 25 s.

El fork usa R=3 para los subtítulos sin tener que configurar nada (el
dictado con el atajo sigue en R=13); `HEX_STREAM_ATT_RIGHT` fija otro valor
para todo. Con R=3, a través de Hex los parciales llegan cada 0,30 s en vez
de cada 1,16 s, y las primeras palabras a los 0,90 s en vez de 1,20 s. Cómo arrancar el fork y usar
`--discovery` con él: [plan](../../docs/plans/teleprompter.md), Fases 0 y 4.

## Distinguir quién habla (`--speakers`)

Con `--speakers`, cada frase lleva el nombre de quien la dijo. En la página,
cada turno empieza en una línea nueva con su nombre en color; el `.srt` y el
`.txt` también lo incluyen.

```powershell
py -m pip install transcribe-cpp                  # una vez
py captions.py --speakers --open                  # "Hablante 1", "Hablante 2"…
py captions.py --names "Iker,Ana" --open          # nombres por orden de aparición
py captions.py --enroll Iker=iker.wav --open      # tu voz, reconocida siempre como "Iker"
```

- **Modelo**: Sortformer v2.1 de NVIDIA, el mismo artefacto que eligió
  upstream para su Lab "Speakers in meetings". Funciona con transcribe.cpp, el
  motor que ya usa Hex, pero en este proceso: Hex no cambia. La primera vez
  se descargan 139 MB, que se comprueban por SHA-256, en
  `%LOCALAPPDATA%\MicroPrompt\models`. También se puede pasar una ruta:
  `--speakers C:\ruta\modelo.gguf`. Por defecto corre en la GPU (Vulkan en
  tu RTX); `--speakers-backend cpu` lo fuerza en la CPU.
- **Cuándo aparece la etiqueta**: al cerrarse cada frase (en la pausa) se
  diariza el audio y la frase recibe la voz que más suena en ella. Mientras
  hablas, la línea en curso toma la voz provisional en cuanto el modelo
  reconoce a alguien conocido, como mucho cada 1,5 s.
- **Registrar tu voz** (`--enroll NOMBRE=fichero.wav`): basta un WAV de 5 a
  10 s con sólo tu voz (graba con la grabadora de Windows y exporta a WAV).
  Esa voz siempre se llama así, aunque pases minutos sin hablar. Se puede
  repetir para varias personas.
- **Página**: `?who=0` oculta los nombres y `?tint=1` pinta el texto de cada
  turno con el color de su hablante.

### Cómo funciona

transcribe.cpp diariza grabaciones enteras; no tiene una API incremental. El
script diariza el **bloque actual**: todo el audio desde un punto fijo hasta
ahora, con un máximo de `--speakers-block` segundos (120 por defecto).
Sortformer numera las voces por orden de llegada, así que dentro de un bloque
cada pasada coincide con la anterior.

Al llenarse un bloque empieza otro, que comparte los últimos 60 s con el
anterior. Las voces que hablaron en esa parte compartida conservan su número
por coincidencia. Las demás voces conocidas se ponen delante del bloque
nuevo, con unos segundos de su propia voz, para que el modelo las reconozca
si vuelven.

Las frases que se acumulan mientras una pasada está en marcha se etiquetan
todas juntas en la siguiente.

### Medido aquí (CPU, sin GPU, voces reales)

Conversaciones montadas con voces reales de los `samples/` de transcribe.cpp
(JFK y hablantes de FLEURS en español, alemán y japonés), con turnos de
2,5 a 4 s y pausas de 0,7 s. Hex está simulado (`mock_hex.py`) y Sortformer
es el real:

| Conversación | Voces | Bloque / solape | Turnos bien | Pasada (mediana) |
| --- | ---: | --- | ---: | ---: |
| 62 s, 16 turnos | 4 | 120 s / 60 s | 16 de 16 | 1,6 s |
| 132 s, 32 turnos; una voz vuelve tras ~110 s | 4 | 120 s / 60 s | 32 de 32 | 2,7 s |
| 266 s, 64 turnos; una voz vuelve tras ~4 min | 4 | 120 s / 60 s | 64 de 64 | 3,4 s |
| 266 s, 64 turnos | 4 | 60 s / 30 s | 50 de 64 | 1,8 s |

Bloques más cortos salen peores: cortan la conversación más a menudo, y el
modelo se confunde al empezar cada bloque. No bajes `--speakers-block` de
120 s salvo que la GPU no dé abasto. Son voces de grabaciones distintas
montadas por turnos, no una conversación real en una sala con un solo
micrófono; tu prueba en Windows es la que cuenta.

Sin GPU, cada pasada sobre 120 s de audio tarda unos 5 s en la CPU usada
aquí (unas 22 veces el tiempo real). En tu RTX debería ser mucho más rápido,
pero aún no se ha medido: el resumen final del script lo indica (`diarización:
… s por pasada`).

### Límites

- Sortformer v2.1 distingue **hasta 4 voces a la vez**. Con más, mezcla dos.
- Una frase en la que hablan dos personas sin pausa entre ellas se atribuye a
  quien más habla.
- Las voces se reconocen dentro de una sesión. Para reconocerte en todas,
  registra tu voz con `--enroll`.
- Es audio de un solo micrófono. Si cada persona tiene su propio micrófono,
  separar por canal sería exacto y no necesitaría modelo (está en el plan).

## Límites (los de `/streams`)

- Sólo hay un stream a la vez. Un segundo cliente recibe "Ya hay otro
  stream".
- **El dictado tiene prioridad**: si usas `Ctrl+Space` mientras corren los
  subtítulos, se pausan y luego se ponen al día (Hex guarda hasta 120 s de
  audio pendiente).
- Hex corta en las pausas, en ventanas de hasta 25 s. Cada ventana termina en
  una frase completa (`segment`).
- Sin GPU, o con un modelo sin streaming, no hay parciales: cada frase
  aparece entera tras la pausa.
- Si Hex no recibe audio durante 10 s, cierra el stream. Mientras el script
  corre envía audio continuamente (también el silencio), así que no pasa.

## Problemas comunes

| Mensaje | Qué hacer |
| --- | --- |
| `No encuentro …\local-api.json` | Abre Hex. |
| `Hex no responde en el puerto…` | Hex se cerró o reinició: vuelve a lanzar el script. |
| `no tiene un modelo cargado` | Pon Hex en *Enabled* y espera a que el modelo esté listo. |
| `distinto de lo pedido` | Hex tiene otro modelo o idioma: cámbialo en Hex o quita `--model`/`--language`. |
| `parciales NO` | El modelo no hace streaming (¿Parakeet v3?) o Hex usa la CPU: elige Nemotron 3.5 ASR y revisa *Speech engine*. |
| `Ya hay otro stream` | Cierra el otro cliente o espera 10 s. |
| `No puedo usar el micrófono` | `py -m pip install sounddevice`, y en Windows › Privacidad › Micrófono permite el acceso a las apps de escritorio. |

## Probar sin Hex

`mock_hex.py` es un Hex falso que sigue el contrato de `/streams` y
"transcribe" por energía con un texto fijo. Sirve para ver la página y
probar cambios sin modelo ni micrófono:

```powershell
py mock_hex.py --discovery mock.json          # en una terminal
py captions.py --discovery mock.json --wav prueba.wav --open   # en otra
py -m unittest -v test_captions.py            # pruebas automáticas
```

Con `--wav` se reproduce un WAV en tiempo real (o `--speed N` veces más
rápido) en lugar del micrófono. Así cada prueba es repetible con la misma
grabación, también contra el Hex real.
