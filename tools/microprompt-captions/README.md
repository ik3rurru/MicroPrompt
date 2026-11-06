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

`http://127.0.0.1:8765/` admite opciones en la URL:

| Opción | Qué hace | Por defecto |
| --- | --- | --- |
| `size` | tamaño de letra en px | `56` |
| `lines` | líneas visibles | `3` |
| `align` | `center` o `left` | `center` |
| `position` | `bottom`, `middle` o `top` | `bottom` |
| `width` | ancho del bloque, en % de la ventana | `90` |
| `fg`, `dim`, `bg` | colores del texto, de las palabras provisionales y del fondo (`#` se escribe `%23`) | blanco, gris, negro |
| `font`, `weight` | familia y grosor de letra | fuente del sistema, `600` |
| `transparent=1` | fondo transparente con contorno, para OBS | |
| `status=0` | oculta la línea de estado | |

Ejemplos:

- **OBS**: añade una fuente *Navegador* (Browser Source) con
  `http://127.0.0.1:8765/?transparent=1&status=0&size=64&lines=2` y su ancho y
  alto de lienzo.
- **Segunda pantalla**: abre la página y pulsa `F11`.
- **Ventana pequeña siempre encima**: abre una ventana sin barras con
  `Start-Process msedge '--app=http://127.0.0.1:8765/?size=40&lines=2'` y
  fíjala encima con PowerToys › *Always On Top* (`Win+Ctrl+T`). El overlay
  nativo, que deja pasar los clics, llegará en la Fase 2.

Las palabras ya fijadas salen en blanco, las provisionales en gris, y las
frases terminadas se atenúan un poco y suben.

## Qué medir en esta fase

1. Leer un párrafo en voz alta a ritmo normal (`--srt`, para revisar la
   calidad después).
2. Hablar libremente, con pausas de 2 a 10 s.
3. La misma prueba con ruido de fondo (música baja, ventilador).
4. Para comparar: en inglés con **Parakeet Unified English**.

Anota el resumen de latencia de cada sesión y los errores de transcripción
más frecuentes en español. Con esos datos se decide la Fase 4 del plan
(chunks más cortos, ajuste de pausas o el modo "local agreement").

Para probar chunks más cortos (afecta a Parakeet, y probablemente a
Nemotron, cuyo artefacto también declara arquitectura `parakeet`), arranca
Hex desde PowerShell con la variable puesta:

```powershell
hex quit
$env:HEX_STREAM_CHUNK_MS = "560"; hex app
```

En la RTX 5070 Ti, el equipo de Hex vio que chunks de 560 y 160 ms causaban
más errores y hasta un 20 % de GPU, sin acelerar el final del dictado
(`docs/features/dictation.md`). Para subtítulos lo que cuenta es la latencia
de los parciales, que aún no se ha medido.

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
