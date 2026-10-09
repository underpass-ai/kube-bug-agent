# kube-bug-agent Helm Chart

Installs the read-only investigator and, optionally, the independent Envoy
backpressure gateway in the release namespace. Each agent has one StatefulSet
replica and its own SQLite database. The chart does not install an LLM, inject
application sidecars, deploy a backend, or modify existing workloads.

## Requirements

- Kubernetes >=1.33 and Helm >=3.19.
- A project image built from this repository containing both binaries. The
  chart does not assume that a public image has been published.
- A block-backed StorageClass with a local filesystem suitable for SQLite WAL,
  or existing ReadWriteOnce PVCs. Do not use NFS or share a database with another
  release. The storage provider must support the pod fsGroup 10001.
- An existing collector authentication Secret when the investigator is enabled.
- An existing HTTP backend Service when backpressure is enabled.

## Install the Investigator

Build and push the image to a registry your cluster can access. Replace the
example repository with your registry; build for the node architecture. For a
local cluster, load the image into its nodes instead of pushing it.

```sh
IMAGE_REPOSITORY=registry.example.com/your-team/kube-bug-agent
docker build -t "$IMAGE_REPOSITORY:0.1.0" .
docker push "$IMAGE_REPOSITORY:0.1.0"
```

Create the namespace and authentication Secret once. This generates the token
without placing it in shell arguments, Helm values, or a source file. Reuse this
Secret on upgrades; do not regenerate it while sidecars still use the old token.
Secrets are not created or managed by the chart.

```sh
kubectl create namespace bug-agent --dry-run=client -o yaml | kubectl apply -f -
openssl rand -hex 32 | tr -d '\n' | kubectl create secret generic bug-agent-auth \
  --namespace bug-agent --from-file=token=/dev/stdin
helm upgrade --install bug-agent ./charts/kube-bug-agent \
  --namespace bug-agent \
  --set image.repository="$IMAGE_REPOSITORY" --set-string image.tag=0.1.0 \
  --wait --timeout 5m
```

Use `imagePullSecrets` for a private registry. `image.digest` takes precedence
over the tag. Envoy is pinned separately by `backpressure.envoy.digest`; clear
that digest to select a different tag, then revalidate metric compatibility.
The default installation collects incidents without LLM analysis;
`investigator.llm.baseUrl` is empty so it does not try to reach this PC from a pod.

```sh
kubectl rollout status statefulset/bug-agent-collector --namespace bug-agent
kubectl port-forward service/bug-agent-collector 8787:8787 --namespace bug-agent
```

`/healthz` and `/readyz` do not require authentication; `/v1/*` requires the shared
Bearer token. The Service is ClusterIP only. Use your cluster TLS/mesh and network
policies where needed; this chart does not install an ingress or TLS termination.

## Enable LLM Analysis

Use an OpenAI-compatible endpoint reachable from the collector pod, including
`/v1`. Configure a model or leave it empty for discovery. Credentials belong in
an existing Secret referenced by `investigator.llm.existingSecret` and
`investigator.llm.key`, not in values. A local llama.cpp server needs no API key.

```sh
helm upgrade bug-agent ./charts/kube-bug-agent --namespace bug-agent \
  --reuse-values --set investigator.llm.baseUrl=http://llm.ai.svc:8080/v1 \
  --set investigator.llm.disableThinking=true --wait --timeout 5m
```

`disableThinking` is a llama.cpp/Qwen extension. Leave it false for providers
that do not support it. This endpoint example does not create a model server.
Neither readiness nor installation success verifies LLM inference.

## Connect Application Sidecars

The investigator observes only labeled resources in its release namespace,
using `bug-agent.io/enabled=true` by default. Add this label to the Deployment
and pod template of workloads to observe. It cannot observe other namespaces.

Application log collection requires a separately configured sidecar:

- Share a JSONL log volume with the application and give the sidecar a writable
  spool volume. It cannot read application stdout automatically.
- Set `COLLECTOR_URL` to `http://bug-agent-collector.bug-agent.svc:8787` for the
  release and namespace above, and reference the same authentication Secret.
- Set `DEPLOYMENT_NAME` and use the downward API for `POD_NAMESPACE`, `POD_NAME`,
  and `POD_UID`. Kubernetes observation enriches owner UID and revision.
- Run the sidecar as UID/GID 10001 with no Kubernetes credentials, no additional
  capabilities, and a read-only root filesystem.

The repository [sidecar manifest example](https://github.com/underpass-ai/kube-bug-agent/blob/main/deploy/kubernetes.yaml)
contains the container, environment, and volume configuration. Adapt the
application-sidecar portion to your workload and chart endpoint; do not apply
the entire demo manifest alongside this chart, as it installs another collector.
Setting `investigator.kubernetes.enabled=false` removes read RBAC and service
account token mounting, leaving an HTTP-only collector without topology enrichment.

## Install Backpressure

Enable it alongside the investigator:

```sh
helm upgrade bug-agent ./charts/kube-bug-agent --namespace bug-agent \
  --reuse-values --set backpressure.enabled=true \
  --set backpressure.backend.host=orders-backend \
  --set backpressure.backend.port=8000 --wait --timeout 5m
```

Or install only the mitigator in the backend namespace; this mode does not
require a collector Secret, Kubernetes credentials, or read RBAC:

```sh
helm upgrade --install pressure ./charts/kube-bug-agent \
  --namespace orders --create-namespace \
  --set image.repository="$IMAGE_REPOSITORY" --set-string image.tag=0.1.0 \
  --set investigator.enabled=false --set backpressure.enabled=true \
  --set backpressure.backend.host=orders-backend \
  --set backpressure.backend.port=8000 --wait --timeout 5m
```

Send protected traffic to `pressure-backpressure.orders.svc:8000`, not directly
to the backend. The chart does not reroute callers or change existing Services.
Only HTTP is supported; no upstream TLS configuration is included. Envoy admin
stays at `127.0.0.1:9901` inside the pod and is absent from every Service.

Configure limits through `backpressure.policy`, Envoy queue/connection limits
and timeouts through `backpressure.envoy`, and the gateway port through
`backpressure.service.port`. `backpressure.policy.dryRun=true` audits proposals
without changing runtime. Initial concurrency must be within the minimum and
maximum. Invalid values and direct gateway self-loops fail before installation.

Excess requests receive 503; clients need bounded retries and rate reduction
for end-to-end backpressure. Limits are per Envoy instance. TCP readiness only
checks that Envoy listens, not backend or mitigator health. Rollouts restart
Envoy and restore initial concurrency; audit intents may remain pending after
interruption. Investigate runtime and history before manual intervention.

## Storage and Upgrades

`investigator.persistence` and `backpressure.persistence` independently support
`size`, `storageClass`, and `existingClaim`. Empty storageClass uses the cluster
default; `"-"` requests no StorageClass for pre-provisioned storage.

Generated PVCs are retained when the StatefulSet is deleted or scaled, including
release uninstall. Existing claims are not owned by the chart. Back up databases
before upgrades; changing claim mode or claim template storage settings can
require StatefulSet recreation and deliberate PVC migration. Do not force-delete
pods, attach a claim to another writer, add HPA, or scale these agents above one.
[Kubernetes PVC retention](https://kubernetes.io/docs/concepts/workloads/controllers/statefulset/#persistentvolumeclaim-retention)
describes the retention policy used by the chart.

```sh
helm uninstall bug-agent --namespace bug-agent
kubectl get pvc --namespace bug-agent
```

Uninstall does not delete separately managed authentication or LLM Secrets.
Changing Secret data requires restarting the collector and its application
sidecars, because credentials are read from environment variables at startup.

## Validate and Package

Local chart tests require Helm >=3.19, Python >=3.9, and PyYAML >=6.0 in addition
to the Rust CI tools. Install PyYAML in your development environment if needed.

```sh
make helm
make ci
make smoke-helm
make helm-package
```

`make helm` runs strict linting and structured render tests across independent
and combined modes, credentials, storage, policy, security, naming, and invalid
values. `make smoke-helm` validates a customized bootstrap with real Envoy through
Docker. Set `KUBECONFORM` to its executable path to additionally validate every
rendered manifest against Kubernetes 1.33 schemas during chart tests.

The package is `artifacts/helm/kube-bug-agent-0.1.0.tgz`, installable with the same
values as the chart directory. Packaging does not publish an OCI artifact, Helm
repository, or container image. Tests do not apply resources to a real cluster.
[Helm schema validation](https://helm.sh/docs/v3/topics/charts/#schema-files)
checks values during lint, template, install, and upgrade.

License: [MIT](https://github.com/underpass-ai/kube-bug-agent/blob/main/LICENSE).
