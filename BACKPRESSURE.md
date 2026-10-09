# Backpressure Mitigation Agent

`backpressure-agent` is a process independent of the `kube-bug-agent`
investigator. It monitors an Envoy HTTP cluster and adjusts its concurrent
request limit. It does not modify Deployments, labels, or dependencies, and does
not require Kubernetes credentials. It does not use the LLM for traffic control;
the investigator retains its OpenAI API connector and read-only permissions.

## Workflow

```text
Clients -> Envoy -> protected backend
              ^
              | max_requests limit, checked after writing
       backpressure-agent -> SQLite: evidence, decision, and outcome
              ^
              | Envoy counters and p95 histogram
```

Traffic that bypasses Envoy is not protected. The limit applies per Envoy
instance, not as a global quota across replicas. This first version controls
request admission: excess requests receive HTTP 503 and `x-envoy-overloaded`.
This is controlled load shedding, not a durable request queue. To propagate
backpressure to the source, clients must reduce their rate and use bounded
retries with backoff and jitter, respecting idempotency.

## Policy

- The first sample establishes a baseline without changing limits.
- Counter deltas are calculated, not percentages accumulated since startup.
  Local overload responses are separated from backend failures; timeouts are
  not counted twice.
- With enough new responses, an error ratio >=10% or p95 >=200 ms halves the
  limit, down to the configured minimum.
- Three consecutive failure-free windows with p95 <150 ms allow an increase
  of one, up to the maximum. Recovery requires available latency data.
- The default cooldown is five seconds. Insufficient traffic, samples without
  timestamp advancement, and counter resets do not authorize recovery.
- A missing or invalid sample does not indicate health. The last known limit
  is retained and the error is logged to stderr.

These values are conservative examples, not an availability guarantee. Adjust
them to the backend latency budget, capacity, and traffic volume. Pods are not
restarted and the application is not scaled in response to each rejection.

## Ports and Adapters

The context lives in `src/backpressure`, with one primary type per file:

| Layer | Responsibility |
| --- | --- |
| Domain | `BackpressureController`, policy, windows, decisions, and validated values |
| Application | `ControlBackpressure`: observe, audit, actuate, and verify |
| `PressureSource` | Read pressure signals |
| `ConcurrencyActuator` | Read and change the selected backend limit |
| `DecisionRepository` | Persisted intents and outcomes |
| Adapters | Envoy admin HTTP and SQLite WAL |
| CLI | Configuration and composition of the separate process |

CLI and JSON fields are converted into `BackendName`, `ConcurrencyLimit`,
`Latency`, `Ratio`, `RequestCount`, and `WindowCount` before entering the domain.
The list of writable keys does not come from a prompt: only
`circuit_breakers.<cluster>.default.max_requests` is written.

## Security and Failures

The admin endpoint only accepts HTTP URLs with a literal loopback IP, without
credentials, a query, or a base path. Redirects are not followed, environment
proxies are ignored, and responses are capped at 1 MiB with a two-second timeout.
The agent does not expose an administration server to the network.

The Envoy configuration includes `admin_layer`, an explicit initial key,
`stats_flush_on_admin: true`, and the compatibility flag that includes active
request rejections in `upstream_rq_pending_overflow`. Do not change these settings
without adapting and verifying metric collection. Only this controller should
consume histogram windows and write the runtime key.

A `pending` intent is persisted before actuation. The limit is checked for
external changes, written, and read back for verification. The outcome is
recorded as `applied`, `observed`, `dry_run`, or `failed`. An audit failure before
actuation blocks the change. An interruption between writing and confirmation
can leave a `pending` intent: it is not blindly replayed at startup.

Envoy admin does not offer atomic compare-and-swap: the precondition check
detects some conflicts, but requires a single writer. If the response to a write
fails, the state may be uncertain; inspect runtime and history. Transactional
rollback between SQLite and Envoy is not promised.

On SIGTERM or Ctrl+C, the process exits and retains the protective limit;
it does not automatically raise it during an outage. Admin overrides are lost
if Envoy restarts, restoring the initial bootstrap value. A new agent starts
with a new baseline. The example deployment uses one replica and `Recreate`;
there is no multi-agent coordination or audit retention yet.

## Local Test

```sh
make ci
docker pull envoyproxy/envoy:v1.39.3
make smoke-backpressure
```

The test starts an HTTP backend that can be saturated and real Envoy on free
loopback ports. It generates bursts, requires a reduction from 8 -> 4 -> 2,
checks that backend failures disappear and excess requests are rejected, then
requires gradual recovery to 3. Processes and scratch files are cleaned up on
exit; the report is written to `artifacts/backpressure-smoke.json`. This Docker
test is not included in the coverage gate; regular tests cover the domain, real
HTTP, SQLite, and SIGTERM.

To use it with your own backend at `127.0.0.1:18081`, Envoy listens on
`127.0.0.1:18080` and its admin on `127.0.0.1:9901`:

```sh
docker run --rm --network host --user 10001:10001 --entrypoint envoy \
  --read-only --cap-drop ALL --security-opt no-new-privileges --tmpfs /tmp \
  --mount type=bind,source="$(pwd)/deploy/backpressure/envoy.json",target=/etc/envoy/envoy.json,readonly \
  envoyproxy/envoy:v1.39.3 -c /etc/envoy/envoy.json \
  --concurrency 1 --disable-hot-restart
```

In another process:

```sh
cargo run --bin backpressure-agent -- run --cluster orders \
  --envoy-admin http://127.0.0.1:9901 --database data/backpressure.db
cargo run --bin backpressure-agent -- history --database data/backpressure.db
```

`--dry-run` saves proposals without writing runtime. `--once` takes a single
sample: because it does not retain a baseline between processes, it does not
demonstrate mitigation. The load test must exercise the continuous process or
multiple ticks of the same controller. See `run --help` to configure limits.

## Kubernetes

`deploy/backpressure` is a Kustomize example of an Envoy gateway with the
mitigator in a separate container. The investigator remains independently
deployed.

1. Rebuild and publish/load the project image, which now contains both binaries.
   The previous local tag does not contain the mitigator.
2. Set the DNS name and port of the real backend Service in
   `envoy-kubernetes.json`. The example value is `orders-backend:8000` in the
   same namespace.
3. Review timings and limits for that backend and choose a local StorageClass
   compatible with SQLite WAL; do not use NFS or multiple writers.
4. Render and review `kubectl kustomize deploy/backpressure`.
5. Apply it in a test namespace and send traffic to `orders-protected:8000`,
   not directly to the backend.

The admin remains at `127.0.0.1:9901` and is not included in any Service.
Containers do not mount Kubernetes credentials and run as UID 10001, without
capabilities or a writable root filesystem. The example has not been applied
in a real cluster. TCP readiness only confirms that Envoy is listening, not that
the backend or controller is healthy.

## Future Scope

There are no RabbitMQ/Kafka adapters, producer control, HPA, dependency switching,
LLM mitigation plans, or activation by an investigator incident yet. This version
also does not determine whether a 5xx is a functional bug or saturation: the
policy limits its observable impact and retains evidence to investigate the
cause. Streaming and traffic with very few responses require other signals.

References: [Envoy circuit breakers](https://www.envoyproxy.io/docs/envoy/v1.39.3/configuration/upstream/cluster_manager/cluster_circuit_breakers),
[administration](https://www.envoyproxy.io/docs/envoy/v1.39.3/operations/admin),
[statistics](https://www.envoyproxy.io/docs/envoy/v1.39.3/configuration/upstream/cluster_manager/cluster_stats).
