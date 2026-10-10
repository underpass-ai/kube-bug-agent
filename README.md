# kube-bug-agent

An initial Rust agent that detects incidents in applications and Kubernetes
Deployments, persists evidence in SQLite, and proposes a diagnosis using an LLM
compatible with OpenAI Chat Completions. Initial inference uses a local Qwen
model through llama.cpp; no OpenAI key is required. This project is a prototype:
it records observable failures and proposes hypotheses, but does not prove
functional bugs or automatically fix resources.

The architecture, domain model, and workflows are described in
[ARCHITECTURE.md](ARCHITECTURE.md).

The separate `backpressure-agent` mitigator controls HTTP concurrency limits in
Envoy, with gradual recovery and SQLite auditing. Its architecture, real load
test, and deployment are described in [BACKPRESSURE.md](BACKPRESSURE.md).
The investigator retains its read-only behavior.

## Structure

- `src/domain`: the `Incident` aggregate, analysis transitions, and validated
  value objects. Identities, signatures, evidence, counters, and diagnoses have
  dedicated types. Evidence is redacted when constructed.
- `src/application`: use cases for ingesting, collecting, analyzing, and
  delivering observations. They depend on ports and the domain.
- `src/ports`: incident repository, diagnosis provider, observation source,
  outbox, and observation sink.
- `src/adapters`: SQLite, Kubernetes, HTTP, files, and an OpenAI-compatible
  connector. Transport DTOs live in `wire`; each file contains one primary type.
  The log adapter converts JSON into `ApplicationLog`, `LogLevel`, and
  `HttpStatus`; detection rules do not know the file format.
- `src/cli`: configuration and dependency composition.

The aggregate groups errors by owner, revision, container, detector, and
normalized signature. Each occurrence retains an idempotent ID and its evidence.
Reusing an ID with different content is rejected. Analysis moves through pending,
processing, complete, or failed states; it allows three attempts with increasing
backoff and recovers interrupted work when the database is opened.

## Local CI

Requirements: Linux, Rust >=1.89, make, ripgrep, cargo-llvm-cov, and llvm-tools-preview.
Chart checks also require Helm >=3.19, Python >=3.9, and PyYAML >=6.0.

```sh
rustup component add rustfmt clippy llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

```sh
make ci
```

Checks dependencies between layers, one primary type per file, formatting,
Clippy with no warnings, and line coverage >=80%. Tests use local HTTP servers
and real SQLite. Coverage does not depend on the local model and does not exclude
production layers. The report is written to `artifacts/coverage.json`.

```sh
make smoke-local
```

Runs sidecar -> HTTP -> SQLite -> local model -> persisted diagnosis.
Discovers the model at `http://127.0.0.1:8080/v1/models`, validates its response,
and writes `artifacts/local-smoke.json`. Use `LOCAL_LLM_BASE_URL` to select another
local server. This test requires an already running server that supports JSON
mode; it does not download or install models. Test scratch files live in `tmp/`
and are cleaned up on exit. Reports, databases, builds, and local configuration
are excluded from git.

## Run Locally

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

The collector returns an acknowledgment after committing; the sidecar removes
an item from its outbox only after receiving that acknowledgment. Both processes
handle `SIGTERM` and Ctrl+C; during shutdown, the sidecar attempts to deliver
pending items for three seconds and retains the rest in its outbox.

You can also analyze Kubernetes fixtures without a cluster:

```sh
cargo run -- scan --fixture tests/fixtures/deployment-failures.json
cargo run -- analyze --limit 1 --disable-thinking
cargo run -- incidents --namespace demo --deployment orders
```

`collector --namespace demo` enables Kubernetes collection. It uses in-cluster
configuration or the local kubeconfig. It only observes resources labeled
`bug-agent.io/enabled=true` and correlates Pod -> ReplicaSet -> Deployment by UID.
The default interval is 30 seconds, configurable with `--poll-seconds`.

The sidecar processes JSONL logs with `level`, `message`/`msg`, `time`/`timestamp`,
and optionally `status`/`status_code`. It detects errors, panics, and HTTP 5xx.
A `--health-url http://127.0.0.1:8000/health` check produces a finding after
three consecutive failures, with an initial 60-second grace period.

## LLM Connector

- `LLM_BASE_URL` / `--llm-base-url`: base URL including `/v1`.
- `LLM_MODEL` / `--llm-model`: model; when omitted, discovers the first available.
- `LLM_API_KEY`: optional credential read from the environment, never printed or
  written to files. `OPENAI_API_KEY` is not automatically reused.
- `--disable-thinking`: llama.cpp extension for these Qwen tests. Omit it when
  using a provider that does not support `chat_template_kwargs`.

Uses `POST /chat/completions`, JSON mode, and local validation of the schema and
evidence references. The model proposes causes and checks; it does not execute
tools, modify resources, or confirm a functional bug. An LLM failure preserves
the incident and its evidence so analysis can be retried.

## Helm Installation

The [Helm chart](charts/kube-bug-agent/README.md) installs the investigator and
optional backpressure gateway independently or together, with single-writer
SQLite storage, namespace-scoped read RBAC, and existing Secret references.
Build and publish/load the project image and create the authentication Secret
before installing the investigator. The chart does not inject sidecars or
deploy an LLM; its README covers setup, traffic routing, storage, and upgrades.

```sh
make helm
make helm-package
```

`make ci` includes chart linting and render tests. `make smoke-helm` additionally
validates the generated bootstrap with real Envoy. See the chart README for the
installation commands and required values. No real cluster deployment is claimed.

## Kubernetes Manifests

`deploy/kubernetes.yaml` contains an example for Kubernetes >=1.33: a single-replica
collector StatefulSet with a PVC, read-only RBAC, and an application with a native
sidecar. Before applying it:

1. Build and publish/load the `kube-bug-agent:0.1.0` image using the Dockerfile.
2. Create the `bug-demo` namespace and the `bug-agent-auth` Secret, with a `token`
   key containing your own credential. The manifest contains no credentials.
3. Set `bug-agent-llm.LLM_BASE_URL` to an endpoint reachable from the pod.
   `127.0.0.1` inside that pod is not this PC.
4. Choose a block-backed StorageClass with a local filesystem; SQLite WAL does
   not support NFS. Keep a single writer collector.

`AGENT_TOKEN` protects `/v1/*` routes with Bearer authentication; it is required
when listening outside loopback. The example uses internal HTTP: use TLS or a
mesh when the cluster network is untrusted. `/healthz` and `/readyz` are probes
that do not require a token.

## Initial Prototype Limitations

- Kubernetes collection uses polling, not watches; it can miss short-lived
  states. The example manifest has not yet been validated in a real cluster.
- Logs must be in a shared volume; the sidecar does not automatically see the
  application stdout. Invalid lines or lines >32 KiB are discarded.
- The queue is limited to 1000 findings. When full, it stops reading without
  advancing the cursor. Multiple rotations or truncations between polls can
  lose content; deleting the pod deletes a queue hosted in emptyDir.
- The first diagnosis is retained per incident; subsequent occurrences add
  evidence. There is no automatic resolution, retention, or high availability.
- Credential redaction is preventive and does not guarantee detection of all
  sensitive data. Do not send production logs to a provider without reviewing
  their content.

## License

[MIT](LICENSE).

Protocol references: [OpenAI Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create),
[llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md),
[Kubernetes sidecars](https://kubernetes.io/docs/concepts/workloads/pods/sidecar-containers/),
[SQLite WAL](https://www.sqlite.org/wal.html).
