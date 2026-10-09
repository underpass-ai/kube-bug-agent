# kube-bug-agent

Primer agente Rust para detectar incidentes en aplicaciones y Deployments de
Kubernetes, persistir evidencia en SQLite y proponer un diagnostico con un LLM
compatible con OpenAI Chat Completions. La inferencia inicial utiliza el modelo
Qwen local mediante llama.cpp; no necesita una clave de OpenAI. El proyecto es
un prototipo: registra fallos observables y propone hipotesis, no demuestra bugs
funcionales ni corrige recursos automaticamente.

La arquitectura, el modelo de dominio y los flujos estan en
[ARCHITECTURE.md](ARCHITECTURE.md).

## Estructura

- `src/domain`: agregado `Incident`, transiciones de analisis y objetos de valor
  validados. Identidades, firmas, evidencias, contadores y diagnosticos tienen
  tipos propios. Las evidencias se filtran al construirse.
- `src/application`: casos de uso para ingerir, recopilar, analizar y entregar
  observaciones. Dependen de puertos y dominio.
- `src/ports`: repositorio de incidentes, proveedor de diagnostico, fuente de
  observaciones, bandeja de salida y destino de observaciones.
- `src/adapters`: SQLite, Kubernetes, HTTP, archivos y conector OpenAI-compatible.
  Los DTO de transporte estan en `wire`; cada archivo contiene un tipo principal.
  El adaptador de logs traduce JSON a `ApplicationLog`, `LogLevel` y `HttpStatus`;
  las reglas de deteccion no conocen el formato del archivo.
- `src/cli`: configuracion y composicion de dependencias.

El agregado agrupa errores por propietario, revision, contenedor, detector y
firma normalizada. Cada ocurrencia conserva un ID idempotente y su evidencia.
Un mismo ID con otro contenido se rechaza. El analisis pasa por pendiente,
procesando, completo o fallido; admite tres intentos con espera creciente y
recupera trabajos interrumpidos al abrir la base de datos.

## CI local

Requisitos: Linux, Rust >=1.89, make, ripgrep, cargo-llvm-cov y llvm-tools-preview.

```sh
rustup component add rustfmt clippy llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

```sh
make ci
```

Verifica las dependencias entre capas, un tipo principal por archivo, formato,
Clippy sin warnings y cobertura de lineas >=80%. Los tests usan servidores HTTP
locales y SQLite real. La cobertura no depende del modelo local ni excluye capas
de produccion. El informe queda en `artifacts/coverage.json`.

```sh
make smoke-local
```

Ejecuta sidecar -> HTTP -> SQLite -> modelo local -> diagnostico persistido.
Descubre el modelo en `http://127.0.0.1:8080/v1/models`, valida su respuesta y
escribe `artifacts/local-smoke.json`. `LOCAL_LLM_BASE_URL` permite elegir otro
servidor local. Esta prueba necesita un servidor compatible con JSON mode ya
arrancado; no descarga ni instala modelos. El scratch de tests vive en `tmp/`
y se limpia al salir. Los informes, bases de datos, builds y configuraciones
locales estan excluidos de git.

## Ejecutar en local

Terminal 1:

```sh
cargo run -- collector --database data/incidents.db \
  --llm-base-url http://127.0.0.1:8080/v1 --disable-thinking
```

Terminal 2:

```sh
cargo run -- sidecar --logs tests/fixtures/application.jsonl \
  --spool data/spool.db --namespace demo --deployment orders \
  --deployment-uid deployment-1 --revision 2 \
  --pod orders-one --pod-uid pod-1 --once
curl --fail http://127.0.0.1:8787/v1/incidents
```

El colector devuelve la confirmacion despues del commit; el sidecar elimina el
elemento de su bandeja de salida solo despues de recibir esa confirmacion.
Ambos procesos atienden `SIGTERM` y Ctrl+C; el sidecar intenta entregar pendientes
durante tres segundos al apagar y conserva lo restante en su bandeja de salida.

Tambien puedes analizar fixtures de Kubernetes sin un cluster:

```sh
cargo run -- scan --fixture tests/fixtures/deployment-failures.json
cargo run -- analyze --limit 1 --disable-thinking
cargo run -- incidents --namespace demo --deployment orders
```

`collector --namespace demo` activa la recopilacion de Kubernetes. Usa la
configuracion in-cluster o el kubeconfig local. Solo observa recursos con
`bug-agent.io/enabled=true` y relaciona Pod -> ReplicaSet -> Deployment por UID.
El intervalo inicial es 30 segundos, configurable con `--poll-seconds`.

El sidecar procesa logs JSONL con `level`, `message`/`msg`, `time`/`timestamp` y
opcionalmente `status`/`status_code`. Detecta errores, panics y HTTP 5xx. Una
comprobacion `--health-url http://127.0.0.1:8000/health` produce un hallazgo tras
tres fallos consecutivos, con 60 segundos de gracia inicial.

## Conector LLM

- `LLM_BASE_URL` / `--llm-base-url`: URL base que incluye `/v1`.
- `LLM_MODEL` / `--llm-model`: modelo; si se omite, descubre el primero disponible.
- `LLM_API_KEY`: credencial opcional; se lee del entorno, no se imprime ni se
  escribe en archivos. No se reutiliza automaticamente `OPENAI_API_KEY`.
- `--disable-thinking`: extension de llama.cpp para estas pruebas Qwen. Omitir
  al usar un proveedor que no admita `chat_template_kwargs`.

Utiliza `POST /chat/completions`, JSON mode y validacion local del esquema y de
las referencias a evidencia. El modelo propone causas y comprobaciones; no
ejecuta herramientas, modifica recursos ni confirma un bug funcional. El
fallo del LLM conserva el incidente y su evidencia para reintentar el analisis.

## Kubernetes

`deploy/kubernetes.yaml` contiene un ejemplo para Kubernetes >=1.33: colector
StatefulSet de una replica con PVC, RBAC de lectura y una aplicacion con sidecar
nativo. Antes de aplicarlo:

1. Construir y publicar/cargar la imagen `kube-bug-agent:0.1.0` con el Dockerfile.
2. Crear el namespace `bug-demo` y el Secret `bug-agent-auth`, clave `token`, con
   una credencial propia. El manifiesto no contiene credenciales.
3. Configurar `bug-agent-llm.LLM_BASE_URL` con un endpoint accesible desde el pod.
   `127.0.0.1` en ese pod no es este PC.
4. Elegir una StorageClass respaldada por bloque y filesystem local; SQLite WAL
   no admite NFS. Mantener un unico colector escritor.

`AGENT_TOKEN` protege las rutas `/v1/*` con Bearer auth; es obligatorio al escuchar
fuera de loopback. El ejemplo usa HTTP interno: usar TLS o un mesh cuando la red
del cluster no sea de confianza. `/healthz` y `/readyz` son probes sin token.

## Limites del primer prototipo

- La recopilacion de Kubernetes usa polling, no watch; puede perder estados
  breves. El manifiesto de ejemplo aun no se ha validado en un cluster real.
- Los logs deben estar en un volumen compartido; el sidecar no ve el stdout
  de la aplicacion automaticamente. Lineas invalidas o >32 KiB se descartan.
- La cola tiene un limite de 1000 hallazgos. Al llenarse, detiene la lectura
  sin avanzar el cursor. Rotaciones multiples o truncados entre sondeos pueden
  perder contenido; eliminar el pod elimina una cola alojada en emptyDir.
- Se conserva el primer diagnostico por incidente; las siguientes ocurrencias
  suman evidencia. No hay resolucion automatica, retencion ni alta disponibilidad.
- El filtrado de credenciales es preventivo y no garantiza detectar todo dato
  sensible. No enviar logs de produccion a un proveedor sin revisar su contenido.

## Licencia

[MIT](LICENSE).

Referencias de protocolo: [OpenAI Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create),
[llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md),
[Kubernetes sidecars](https://kubernetes.io/docs/concepts/workloads/pods/sidecar-containers/),
[SQLite WAL](https://www.sqlite.org/wal.html).
