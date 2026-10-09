# Agente de mitigacion de backpressure

`backpressure-agent` es un proceso independiente del investigador
`kube-bug-agent`. Supervisa un cluster HTTP de Envoy y ajusta su limite de
peticiones simultaneas. No modifica Deployments, etiquetas o dependencias,
ni necesita credenciales de Kubernetes. No usa el LLM en el control de trafico;
el investigador conserva su conector OpenAI API y sus permisos de lectura.

## Flujo

```text
Clientes -> Envoy -> backend protegido
              ^
              | limite max_requests, comprobado despues de escribir
       backpressure-agent -> SQLite: evidencia, decision y resultado
              ^
              | contadores e histograma p95 de Envoy
```

El trafico que evita Envoy no queda protegido. El limite es por instancia de
Envoy, no una cuota global para varias replicas. La primera version controla
admission de peticiones: el exceso recibe HTTP 503 y `x-envoy-overloaded`.
Eso es rechazo controlado de carga, no una cola durable de peticiones. Para
propagar backpressure al origen, los clientes deben reducir su ritmo y aplicar
reintentos acotados con backoff y jitter, respetando la idempotencia.

## Politica

- La primera muestra establece una linea base, sin modificar limites.
- Se calculan diferencias de contadores, no porcentajes acumulados desde el
  arranque. Las respuestas locales de sobrecarga se separan de los fallos del
  backend; los timeouts no se cuentan dos veces.
- Con suficientes respuestas nuevas, un ratio de errores >=10% o p95 >=200 ms
  reduce el limite a la mitad, hasta el minimo configurado.
- Tres ventanas consecutivas sin fallos y con p95 <150 ms permiten subir una
  unidad, hasta el maximo. La recuperacion requiere latencia disponible.
- El cooldown inicial es cinco segundos. Trafico insuficiente, muestras sin
  avance temporal y reinicios de contadores no autorizan una recuperacion.
- Una muestra ausente o invalida no equivale a salud. Se conserva el ultimo
  limite conocido y se registra el error en stderr.

Estos valores son ejemplos conservadores, no una garantia de disponibilidad.
Hay que ajustarlos al presupuesto de latencia, capacidad y volumen del backend.
No se reinician Pods ni se escala la aplicacion ante cada rechazo.

## Puertos y adaptadores

El contexto vive en `src/backpressure`, con un tipo principal por archivo:

| Capa | Responsabilidad |
| --- | --- |
| Dominio | `BackpressureController`, politica, ventanas, decisiones y valores validados |
| Aplicacion | `ControlBackpressure`: observar, auditar, actuar y verificar |
| `PressureSource` | Lectura de senales de presion |
| `ConcurrencyActuator` | Lectura y cambio del limite del backend seleccionado |
| `DecisionRepository` | Intenciones y resultados persistidos |
| Adaptadores | Envoy admin HTTP y SQLite WAL |
| CLI | Configuracion y composicion del proceso separado |

Los campos de CLI y JSON se convierten a `BackendName`, `ConcurrencyLimit`,
`Latency`, `Ratio`, `RequestCount` y `WindowCount` antes de entrar en el dominio.
La lista de claves modificables no procede de un prompt: solo se escribe
`circuit_breakers.<cluster>.default.max_requests`.

## Seguridad y fallos

El endpoint admin solo admite una IP loopback HTTP literal, sin credenciales,
query ni ruta base. No se siguen redirecciones, se ignoran proxies de entorno y
las respuestas estan acotadas a 1 MiB, con timeout de dos segundos. No se expone
un servidor de administracion del agente a la red.

La configuracion de Envoy contiene `admin_layer`, una clave inicial explicita,
`stats_flush_on_admin: true` y el flag de compatibilidad que incluye los rechazos
de peticiones activas en `upstream_rq_pending_overflow`. No cambiar estos
ajustes sin adaptar y verificar la lectura de metricas. Solo este controlador
debe consumir las ventanas del histograma y escribir la clave de runtime.

Antes de actuar se persiste una intencion `pending`. Se comprueba que el limite
no haya cambiado externamente, se escribe y se verifica su lectura. El resultado
queda como `applied`, `observed`, `dry_run` o `failed`. Un fallo de auditoria antes
de actuar bloquea el cambio. Una interrupcion entre escritura y confirmacion
puede dejar una intencion `pending`: no se reejecuta a ciegas al arrancar.

Envoy admin no ofrece compare-and-swap atomico: la comprobacion previa detecta
parte de los conflictos, pero requiere un unico escritor. Si falla la respuesta
de una escritura, el estado puede ser incierto; revisar runtime e historial.
No se promete rollback transaccional entre SQLite y Envoy.

Al recibir SIGTERM o Ctrl+C, el proceso sale y conserva el limite protector;
no lo eleva automaticamente durante una caida. Los overrides admin se pierden
si reinicia Envoy, que recupera el valor inicial del bootstrap. Un nuevo agente
empieza con una nueva linea base. El despliegue de ejemplo usa una replica y
`Recreate`; aun no hay coordinacion multiagente ni retencion de auditoria.

## Prueba local

```sh
make ci
docker pull envoyproxy/envoy:v1.39.3
make smoke-backpressure
```

La prueba arranca un backend HTTP saturable y Envoy real en puertos loopback
libres. Genera rafagas, exige reduccion 8 -> 4 -> 2, comprueba que desaparezcan
los fallos del backend y que el exceso sea rechazado. Despues exige recuperacion
gradual a 3. Los procesos y scratch se limpian al salir; el informe queda en
`artifacts/backpressure-smoke.json`. No se incluye esta prueba Docker en el gate
de cobertura; los tests normales cubren dominio, HTTP real, SQLite y SIGTERM.

Para usarlo con un backend propio en `127.0.0.1:18081`, Envoy escucha en
`127.0.0.1:18080` y su admin en `127.0.0.1:9901`:

```sh
docker run --rm --network host --user 10001:10001 --entrypoint envoy \
  --read-only --cap-drop ALL --security-opt no-new-privileges --tmpfs /tmp \
  --mount type=bind,source="$(pwd)/deploy/backpressure/envoy.json",target=/etc/envoy/envoy.json,readonly \
  envoyproxy/envoy:v1.39.3 -c /etc/envoy/envoy.json \
  --concurrency 1 --disable-hot-restart
```

En otro proceso:

```sh
cargo run --bin backpressure-agent -- run --cluster orders \
  --envoy-admin http://127.0.0.1:9901 --database data/backpressure.db
cargo run --bin backpressure-agent -- history --database data/backpressure.db
```

`--dry-run` guarda propuestas sin escribir runtime. `--once` toma una sola
muestra: como no conserva una linea base entre procesos, no demuestra una
mitigacion. La prueba de carga debe ejercitar el proceso continuo o varios ticks
de un mismo controlador. Consultar `run --help` para configurar los limites.

## Kubernetes

`deploy/backpressure` es un ejemplo Kustomize de gateway Envoy con el mitigador
como contenedor separado. El investigador sigue desplegandose por su cuenta.

1. Reconstruir y publicar/cargar la imagen del proyecto, que ahora contiene
   ambos binarios. El tag local anterior no contiene el mitigador.
2. Configurar en `envoy-kubernetes.json` el DNS y puerto del Service backend
   real. El valor de ejemplo es `orders-backend:8000` en el mismo namespace.
3. Revisar tiempos y limites para ese backend y elegir una StorageClass local
   compatible con SQLite WAL; no usar NFS ni varios escritores.
4. Renderizar y revisar `kubectl kustomize deploy/backpressure`.
5. Aplicar en un namespace de pruebas y enviar el trafico a
   `orders-protected:8000`, no directamente al backend.

El admin permanece en `127.0.0.1:9901` y no figura en ningun Service. Los
contenedores no montan credenciales de Kubernetes y ejecutan como UID 10001,
sin capabilities ni root filesystem escribible. El ejemplo no se ha aplicado
en un cluster real. La readiness TCP solo confirma que Envoy escucha, no que el
backend o el controlador esten sanos.

## Alcance pendiente

No hay aun adaptadores de RabbitMQ/Kafka, control de productores, HPA, cambio
de dependencia, planes LLM de mitigacion ni activacion por un incidente del
investigador. Tampoco se detecta si un 5xx es un bug funcional o saturacion:
esta politica limita su impacto observable y conserva evidencia para estudiar
la causa. Streaming y trafico con muy pocas respuestas requieren otras senales.

Referencias: [Envoy circuit breakers](https://www.envoyproxy.io/docs/envoy/v1.39.3/configuration/upstream/cluster_manager/cluster_circuit_breakers),
[administracion](https://www.envoyproxy.io/docs/envoy/v1.39.3/operations/admin),
[estadisticas](https://www.envoyproxy.io/docs/envoy/v1.39.3/configuration/upstream/cluster_manager/cluster_stats).
