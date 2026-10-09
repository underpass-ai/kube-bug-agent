# kube-bug-agent Architecture

A Rust prototype that observes incidents in applications and Kubernetes
Deployments, retains evidence in SQLite, and proposes a diagnosis using an LLM.
An observable failure is a fact; a cause suggested by the model is a hypothesis.
The agent does not execute model commands or modify the cluster.

## Processes

The same binary provides two service modes:

- `sidecar`: reads JSONL logs from a shared volume, optionally checks a local
  HTTP endpoint, and delivers observations to the collector.
- `collector`: receives observations, queries Kubernetes by namespace and
  label, persists incidents, and runs pending analyses.

The collector runs outside the observed pod so it can detect failures that
prevent startup, such as image, scheduling, or ReplicaSet creation problems.
The example uses a single collector StatefulSet with a PVC. SQLite is not shared
between multiple writer replicas.

```text
Application -> JSONL -> Sidecar -> SQLite outbox -> HTTP -> Collector
                                                             |
Kubernetes API -> pods / replicasets / deployments / events ---+
                                                             |
                                                             v
                                                       SQLite incidents
                                                             |
                                                             v
                                                       LLM -> Diagnosis
```

## Layers

Dependencies point inward:

```text
CLI / Adapters -> Application -> Ports / Domain
Ports -> contracts using domain types and application queries
Domain -> no other project layer
```

| Directory | Responsibility |
| --- | --- |
| `src/domain` | Aggregate, value objects, identity, and invariants |
| `src/application` | Use cases, queries, and log detection rules |
| `src/ports` | Persistence, observation, delivery, and diagnosis contracts |
| `src/adapters` | SQLite, HTTP, Kubernetes, files, signals, and LLM |
| `src/cli` | Input configuration and dependency composition |

Each Rust file has one primary type: a struct, enum, or trait. Export modules
and validation helpers do not add a second responsibility. DTOs are used at HTTP
and LLM boundaries. The JSON adapter converts each line into `ApplicationLog`,
`LogLevel`, and `HttpStatus`; `LogDetector` receives those types and does not know
the file format.

## Domain Model

The context is incident investigation for a deployed workload.

- `Workload` identifies the namespace, Deployment, revision, pod, and container
  using distinct value objects; unvalidated strings are not passed around.
- `Observation` describes a fact with an ID, timestamp, detector, severity,
  `ErrorSignature`, and `Evidence`. Evidence has a size limit and preventive
  credential redaction.
- `Incident` is the aggregate root. It groups observations by namespace, owner,
  revision, container, detector, and normalized signature.
- `Diagnosis` contains a summary, suspected cause, confidence, checks, and
  evidence references. Fabricated references are rejected.

The fingerprint uses the Deployment UID; if absent, it uses the pod UID.
A new revision produces another incident. Occurrences have idempotent IDs:
repeating the same content does not increase the counter, and reusing an ID with
different content produces a conflict.

The aggregate retains the first event for analysis. Subsequent occurrences
update the counter and time intervals and are stored separately. Analysis
transitions belong to the aggregate, not the LLM adapter:

```text
Pending -> Processing -> Complete
               |
               +-> Pending (retry with increasing backoff)
               +-> Failed  (third attempt)
```

When SQLite is reopened, `Processing` work is recovered for retry or marked
failed if it has already used its last attempt. A model failure never deletes
the original observation.

## Ports and Use Cases

| Port | Initial Adapter |
| --- | --- |
| `IncidentRepository` | `SqliteIncidentRepository` |
| `DiagnosisProvider` | `OpenAiDiagnosisProvider` |
| `ObservationSource` | `KubernetesSource` or `FixtureSource` |
| `ObservationSink` | `HttpObservationSink` |
| `ObservationOutbox` | `SqliteOutbox` |

Use cases receive ports through dependency injection:

- `IngestIncident`: records an observation idempotently.
- `CollectIncidents`: collects a source snapshot and persists findings.
- `AnalyzeIncident`: claims a pending incident, requests a diagnosis, and saves
  the result or analysis failure.
- `FlushOutbox`: delivers pending items and removes each one only after receiving
  a valid acknowledgment from the sink.

## Workflows and Persistence

The sidecar saves the finding and file cursor in one transaction. When the queue
is full, it does not advance the cursor. The collector acknowledges ingestion
after committing the incident and occurrence. If the HTTP response is lost, the
sidecar retries, and the event ID prevents counting the same observation twice.

The collector queries pods, ReplicaSets, Deployments, and Warning events. It
correlates owners by UID and controller references, not similar names. When
Kubernetes observation is enabled, it enriches the sidecar identity using its
pod UID; it returns a retryable error while that identity is not ready.

SQLite uses WAL and transactions for the `incidents` and `occurrences` tables.
The sidecar outbox is a separate database. Both processes handle `SIGTERM` and
Ctrl+C; the sidecar has three seconds to attempt a final delivery.

## Connector and Security

The connector uses `/v1/models` to discover the model when none is configured,
and `/v1/chat/completions` with JSON mode. It validates the schema, value objects,
finish reason, and evidence references. It has a timeout and response limit,
and does not follow redirects. Provider error bodies are not exposed as
diagnostic messages.

Logs are untrusted input, including inside the prompt. The model has no tools.
Credential redaction is a preventive measure, not a guarantee that production
data will be anonymized.

`AGENT_TOKEN` protects `/v1/*` and is required outside loopback. The `/healthz`
and `/readyz` probes do not require a token. The manifest uses read-only RBAC
scoped to one namespace; the sidecar does not need Kubernetes credentials.

## Verification and Limitations

The `backpressure-agent` mitigator is a separate binary and an independent
context in `src/backpressure`, with its own domain, application, ports, and
adapters. It controls Envoy and does not modify the investigator or its RBAC.
The policy and actuation limits are described in [BACKPRESSURE.md](BACKPRESSURE.md).

`make ci` checks dependencies between layers, one primary type per file,
formatting, Clippy, and line coverage >=80%, without excluding production layers.
It also runs Helm linting and structured chart render tests. The chart in
`charts/kube-bug-agent` deploys each enabled agent as a separate single-replica
StatefulSet, with retained database claims and no application-sidecar injection.
Tests cover invariants, real SQLite, local HTTP servers, simulated Kubernetes,
retries, backpressure, rotation, probes, and binary shutdown. `make smoke-local`
adds real inference using a locally installed model.

The prototype uses polling and can miss short-lived states. Multiple rotations
or truncations between reads can lose logs. A queue in `emptyDir` disappears
with the pod. There is no automatic resolution, retention, HA, or validation of
a real Kubernetes deployment. Operational instructions are in
[README.md](README.md).
