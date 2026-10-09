# Arquitectura de kube-bug-agent

Prototipo Rust para observar incidentes de aplicaciones y Deployments de
Kubernetes, conservar evidencia en SQLite y proponer un diagnostico con un LLM.
Un fallo observable es un hecho; una causa sugerida por el modelo es una
hipotesis. El agente no ejecuta comandos del modelo ni modifica el cluster.

## Procesos

Un mismo binario tiene dos modos de servicio:

- `sidecar`: lee logs JSONL desde un volumen compartido, comprueba opcionalmente
  un endpoint HTTP local y entrega observaciones al colector.
- `collector`: recibe observaciones, consulta Kubernetes por namespace y
  etiqueta, persiste incidentes y ejecuta analisis pendientes.

El colector vive fuera del pod observado para poder detectar fallos que impiden
su arranque, como problemas de imagen, scheduling o creacion del ReplicaSet.
El ejemplo usa un unico colector StatefulSet con PVC. SQLite no se comparte
entre multiples replicas escritoras.

```text
Aplicacion -> JSONL -> Sidecar -> outbox SQLite -> HTTP -> Colector
                                                           |
Kubernetes API -> pods / replicasets / deployments / events -+
                                                           |
                                                           v
                                                   Incidentes SQLite
                                                           |
                                                           v
                                                 LLM -> Diagnostico
```

## Capas

Las dependencias apuntan hacia dentro:

```text
CLI / Adaptadores -> Aplicacion -> Puertos / Dominio
Puertos -> contratos con tipos de dominio y consultas de aplicacion
Dominio -> ninguna otra capa del proyecto
```

| Directorio | Responsabilidad |
| --- | --- |
| `src/domain` | Agregado, objetos de valor, identidad e invariantes |
| `src/application` | Casos de uso, consultas y reglas de deteccion de logs |
| `src/ports` | Contratos de persistencia, observacion, entrega y diagnostico |
| `src/adapters` | SQLite, HTTP, Kubernetes, archivos, senales y LLM |
| `src/cli` | Configuracion de entrada y composicion de dependencias |

Cada archivo Rust tiene un tipo principal: struct, enum o trait. Los modulos de
exportacion y auxiliares de validacion no anaden una segunda responsabilidad.
Los DTO se usan en las fronteras HTTP y LLM. El adaptador JSON convierte cada
linea a `ApplicationLog`, `LogLevel` y `HttpStatus`; `LogDetector` recibe esos
tipos y no conoce el formato del archivo.

## Modelo de dominio

El contexto es la investigacion de incidentes de un workload desplegado.

- `Workload` identifica namespace, Deployment, revision, pod y contenedor con
  objetos de valor distintos; no se intercambian strings sin validar.
- `Observation` describe un hecho con ID, instante, detector, severidad,
  `ErrorSignature` y `Evidence`. Las evidencias tienen un limite de tamano y
  filtrado preventivo de credenciales.
- `Incident` es la raiz del agregado. Agrupa observaciones por namespace,
  propietario, revision, contenedor, detector y firma normalizada.
- `Diagnosis` contiene resumen, causa sospechada, confianza, comprobaciones y
  referencias a la evidencia. Una referencia inventada se rechaza.

El fingerprint usa el UID del Deployment; si falta, usa el UID del pod. Una
nueva revision produce otro incidente. Las ocurrencias tienen IDs idempotentes:
repetir el mismo contenido no aumenta el contador, y reutilizar un ID con otro
contenido produce un conflicto.

El agregado conserva el primer evento para el analisis. Las siguientes
ocurrencias actualizan contador e intervalos y se almacenan por separado.
Las transiciones de analisis pertenecen al agregado, no al adaptador LLM:

```text
Pending -> Processing -> Complete
               |
               +-> Pending (reintento con espera creciente)
               +-> Failed  (tercer intento)
```

Al reabrir SQLite, un trabajo `Processing` se recupera para reintento o se marca
fallido si ya consumio el ultimo intento. El fallo del modelo nunca elimina la
observacion original.

## Puertos y casos de uso

| Puerto | Adaptador inicial |
| --- | --- |
| `IncidentRepository` | `SqliteIncidentRepository` |
| `DiagnosisProvider` | `OpenAiDiagnosisProvider` |
| `ObservationSource` | `KubernetesSource` o `FixtureSource` |
| `ObservationSink` | `HttpObservationSink` |
| `ObservationOutbox` | `SqliteOutbox` |

Los casos de uso reciben los puertos por inyeccion de dependencias:

- `IngestIncident`: registra una observacion de manera idempotente.
- `CollectIncidents`: recoge una instantanea de la fuente y persiste hallazgos.
- `AnalyzeIncident`: reclama un incidente pendiente, solicita el diagnostico y
  guarda el resultado o el fallo de analisis.
- `FlushOutbox`: entrega pendientes y elimina cada elemento solo tras recibir
  una confirmacion valida del destino.

## Flujos y persistencia

El sidecar guarda en una transaccion el hallazgo y el cursor del archivo. Cuando
la cola se llena, no avanza el cursor. El colector confirma la ingesta despues
del commit de incidente y ocurrencia. Si se pierde la respuesta HTTP, el sidecar
reintenta y el ID del evento evita contar dos veces la misma observacion.

El colector consulta pods, ReplicaSets, Deployments y eventos Warning. Relaciona
propietarios por UID y referencias controller, no por nombres parecidos. Con la
observacion Kubernetes activada, enriquece la identidad del sidecar usando su
UID de pod; devuelve un error reintentable mientras esa identidad no esta lista.

SQLite usa WAL y transacciones para las tablas `incidents` y `occurrences`. El
outbox del sidecar es otra base de datos. Ambos procesos atienden `SIGTERM` y
Ctrl+C; el sidecar dispone de tres segundos para intentar una ultima entrega.

## Conector y seguridad

El conector utiliza `/v1/models` para descubrir el modelo cuando no se configura
uno y `/v1/chat/completions` con JSON mode. Valida el esquema, los objetos de
valor, el motivo de finalizacion y las referencias de evidencia. Tiene timeout,
limite de respuesta y no sigue redirecciones. Los cuerpos de error del proveedor
no se exponen como mensajes de diagnostico.

Los logs son entrada no confiable, tambien dentro del prompt. El modelo no
tiene herramientas. El filtrado de credenciales es una proteccion preventiva,
no una garantia de anonimizar datos de produccion.

`AGENT_TOKEN` protege `/v1/*` y es obligatorio fuera de loopback. Los probes
`/healthz` y `/readyz` no requieren token. El manifiesto usa RBAC de lectura
limitado a un namespace; el sidecar no necesita credenciales de Kubernetes.

## Verificacion y limites

El mitigador `backpressure-agent` es otro binario y un contexto independiente
en `src/backpressure`, con sus propias capas de dominio, aplicacion, puertos y
adaptadores. Controla Envoy, no modifica el investigador ni su RBAC. La politica
y los limites de actuacion estan en [BACKPRESSURE.md](BACKPRESSURE.md).

`make ci` comprueba dependencias entre capas, un tipo principal por archivo,
formato, Clippy y cobertura de lineas >=80%, sin excluir capas de produccion.
Los tests cubren invariantes, SQLite real, servidores HTTP locales, Kubernetes
simulado, reintentos, backpressure, rotacion, probes y apagado del binario.
`make smoke-local` anade inferencia real con un modelo instalado localmente.

El prototipo usa polling y puede perder estados breves. Rotaciones multiples o
truncados entre lecturas pueden perder logs. Una cola en `emptyDir` desaparece
con el pod. No hay resolucion automatica, retencion, HA ni validacion de un
despliegue real en Kubernetes. Las instrucciones operativas estan en
[README.md](README.md).
