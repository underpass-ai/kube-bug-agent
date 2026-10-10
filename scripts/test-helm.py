import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[1]
CHART = ROOT / "charts" / "kube-bug-agent"


class HelmChartTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if os.environ.get("KUBECONFORM"):
            scratch = ROOT / "tmp"
            scratch.mkdir(exist_ok=True)
            cls.schema_cache = tempfile.TemporaryDirectory(dir=scratch)

    @classmethod
    def tearDownClass(cls):
        if os.environ.get("KUBECONFORM"):
            cls.schema_cache.cleanup()

    def render(self, overrides=None, release="bug-agent", namespace="demo"):
        result = subprocess.run(
            ["helm", "template", release, str(CHART), "--namespace", namespace,
             "--kube-version", "1.33.0", "--values", "-"],
            input=yaml.safe_dump(overrides or {}), text=True, capture_output=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        if os.environ.get("KUBECONFORM"):
            validation = subprocess.run(
                [os.environ["KUBECONFORM"], "-strict", "-kubernetes-version", "1.33.0",
                 "-cache", self.schema_cache.name],
                input=result.stdout, text=True, capture_output=True, timeout=120,
            )
            self.assertEqual(validation.returncode, 0, validation.stdout + validation.stderr)
        return [document for document in yaml.safe_load_all(result.stdout) if document]

    def resource(self, documents, kind, name):
        return next(item for item in documents
                    if item["kind"] == kind and item["metadata"]["name"] == name)

    def backpressure(self, **overrides):
        return {"backpressure": {"enabled": True, "backend": {"host": "orders-backend"}, **overrides}}

    def envoy(self, documents, prefix="bug-agent"):
        return json.loads(self.resource(
            documents, "ConfigMap", f"{prefix}-backpressure-envoy"
        )["data"]["envoy.json"])

    def test_lint_default_and_combined_modes(self):
        for args in [[], ["--set", "backpressure.enabled=true",
                         "--set", "backpressure.backend.host=orders-backend"]]:
            with self.subTest(args=args):
                result = subprocess.run(
                    ["helm", "lint", "--strict", str(CHART), *args],
                    text=True, capture_output=True, timeout=30,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_default_installation_is_read_only_and_namespace_scoped(self):
        documents = self.render(namespace="production")
        self.assertEqual(len(documents), 6)
        self.assertFalse(any(item["kind"] in {"Secret", "Namespace", "ClusterRole"}
                             for item in documents))
        role = self.resource(documents, "Role", "bug-agent-reader")
        self.assertEqual(role["rules"], [
            {"apiGroups": [""], "resources": ["pods", "events"], "verbs": ["get", "list"]},
            {"apiGroups": ["apps"], "resources": ["deployments", "replicasets"], "verbs": ["get", "list"]},
        ])
        binding = self.resource(documents, "RoleBinding", "bug-agent-reader")
        self.assertEqual(binding["subjects"][0]["namespace"], "production")
        pod = self.resource(documents, "StatefulSet", "bug-agent-collector")["spec"]["template"]["spec"]
        self.assertTrue(pod["automountServiceAccountToken"])
        collector = pod["containers"][0]
        self.assertEqual(collector["args"][collector["args"].index("--namespace") + 1], "production")
        env = {item["name"]: item for item in collector["env"]}
        self.assertEqual(env["AGENT_TOKEN"]["valueFrom"]["secretKeyRef"],
                         {"name": "bug-agent-auth", "key": "token"})
        self.assertNotIn("LLM_BASE_URL", env)

    def test_backpressure_can_run_alone_without_kubernetes_permissions(self):
        values = self.backpressure()
        values["investigator"] = {"enabled": False}
        documents = self.render(values)
        self.assertEqual(len(documents), 4)
        self.assertFalse(any(item["kind"] in {"Role", "RoleBinding", "ServiceAccount", "Secret"}
                             for item in documents))
        pod = self.resource(documents, "StatefulSet", "bug-agent-backpressure")["spec"]["template"]["spec"]
        self.assertFalse(pod["automountServiceAccountToken"])
        self.assertEqual([item["name"] for item in pod["containers"]], ["envoy", "mitigator"])

    def test_security_storage_and_service_selectors_in_combined_mode(self):
        documents = self.render(self.backpressure())
        self.assertEqual(len(documents), 10)
        workloads = [item for item in documents if item["kind"] == "StatefulSet"]
        for workload in workloads:
            spec = workload["spec"]
            self.assertEqual(spec["replicas"], 1)
            self.assertEqual(spec["persistentVolumeClaimRetentionPolicy"],
                             {"whenDeleted": "Retain", "whenScaled": "Retain"})
            labels = spec["template"]["metadata"]["labels"]
            self.assertEqual(spec["selector"]["matchLabels"], labels)
            self.assertEqual(self.resource(documents, "Service", spec["serviceName"])["spec"]["clusterIP"], "None")
            pod = spec["template"]["spec"]
            self.assertEqual(pod["securityContext"], {
                "runAsNonRoot": True, "runAsUser": 10001, "runAsGroup": 10001,
                "fsGroup": 10001, "seccompProfile": {"type": "RuntimeDefault"},
            })
            volume_names = {item["metadata"]["name"] for item in spec["volumeClaimTemplates"]}
            volume_names.update(item["name"] for item in pod.get("volumes", []))
            for container in pod["containers"]:
                self.assertEqual(container["securityContext"], {
                    "allowPrivilegeEscalation": False, "readOnlyRootFilesystem": True,
                    "capabilities": {"drop": ["ALL"]},
                })
                self.assertTrue(all(item["name"] in volume_names for item in container["volumeMounts"]))
        for service in [item for item in documents if item["kind"] == "Service"]:
            matching = [item for item in workloads if item["spec"]["selector"]["matchLabels"] == service["spec"]["selector"]]
            self.assertEqual(len(matching), 1)
            ports = {port["name"] for container in matching[0]["spec"]["template"]["spec"]["containers"]
                     for port in container.get("ports", [])}
            self.assertTrue(all(port["targetPort"] in ports for port in service["spec"]["ports"]))
            self.assertNotIn(9901, [item["port"] for item in service["spec"]["ports"]])

    def test_envoy_and_controller_use_the_same_custom_policy(self):
        values = self.backpressure(
            cluster="payments-v2", backend={"host": "payments.prod.svc.cluster.local", "port": 9000},
            service={"port": 9001}, envoy={"routeTimeout": "3s", "maxPendingRequests": 1},
            policy={"initialConcurrency": 4, "minConcurrency": 2, "maxConcurrency": 9,
                    "latencyMs": 250, "errorRatio": 0.2, "minSamples": 20,
                    "healthyWindows": 4, "cooldownSeconds": 7, "pollSeconds": 3, "dryRun": True},
        )
        documents = self.render(values)
        config = self.envoy(documents)
        self.assertEqual(config["admin"]["address"]["socket_address"], {"address": "127.0.0.1", "port_value": 9901})
        self.assertTrue(config["stats_flush_on_admin"])
        runtime = config["layered_runtime"]["layers"][0]["static_layer"]
        self.assertEqual(runtime["circuit_breakers.payments-v2.default.max_requests"], 4)
        self.assertFalse(runtime["envoy.reloadable_features.skip_pending_overflow_count_on_active_rq"])
        self.assertEqual(config["layered_runtime"]["layers"][1]["admin_layer"], {})
        cluster = config["static_resources"]["clusters"][0]
        self.assertEqual(cluster["name"], "payments-v2")
        self.assertEqual(cluster["load_assignment"]["endpoints"][0]["lb_endpoints"][0]["endpoint"]["address"]["socket_address"],
                         {"address": "payments.prod.svc.cluster.local", "port_value": 9000})
        self.assertEqual(cluster["circuit_breakers"]["thresholds"][0]["max_pending_requests"], 1)
        manager = config["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]["typed_config"]
        self.assertEqual(manager["route_config"]["virtual_hosts"][0]["routes"][0]["route"], {"cluster": "payments-v2", "timeout": "3s"})
        pod = self.resource(documents, "StatefulSet", "bug-agent-backpressure")["spec"]["template"]
        encoded = self.resource(documents, "ConfigMap", "bug-agent-backpressure-envoy")["data"]["envoy.json"].rstrip()
        self.assertEqual(pod["metadata"]["annotations"]["checksum/envoy"], hashlib.sha256(encoded.encode()).hexdigest())
        args = pod["spec"]["containers"][1]["args"]
        for flag, value in {"--cluster": "payments-v2", "--min-concurrency": "2", "--max-concurrency": "9",
                            "--latency-ms": "250", "--error-ratio": "0.2", "--min-samples": "20",
                            "--healthy-windows": "4", "--cooldown-seconds": "7", "--poll-seconds": "3"}.items():
            self.assertEqual(args[args.index(flag) + 1], value)
        self.assertIn("--dry-run", args)
        self.assertEqual(self.resource(documents, "Service", "bug-agent-backpressure")["spec"]["ports"][0]["port"], 9001)

    def test_existing_claims_and_storage_classes(self):
        for component, resource_name in [("investigator", "bug-agent-collector"), ("backpressure", "bug-agent-backpressure")]:
            for storage_class in ["", "fast-local", "-"]:
                with self.subTest(component=component, storage_class=storage_class):
                    values = self.backpressure()
                    values.setdefault(component, {})["persistence"] = {"storageClass": storage_class, "size": "2Gi"}
                    spec = self.resource(self.render(values), "StatefulSet", resource_name)["spec"]
                    claim = spec["volumeClaimTemplates"][0]["spec"]
                    self.assertEqual(claim["resources"]["requests"]["storage"], "2Gi")
                    if storage_class:
                        self.assertEqual(claim["storageClassName"], "" if storage_class == "-" else storage_class)
                    else:
                        self.assertNotIn("storageClassName", claim)
            values = self.backpressure()
            values.setdefault(component, {})["persistence"] = {"existingClaim": f"{component}-data"}
            spec = self.resource(self.render(values), "StatefulSet", resource_name)["spec"]
            self.assertNotIn("volumeClaimTemplates", spec)
            self.assertIn({"name": "audit" if component == "backpressure" else "database",
                           "persistentVolumeClaim": {"claimName": f"{component}-data"}}, spec["template"]["spec"]["volumes"])

    def test_images_resources_and_scheduling_are_configurable(self):
        values = self.backpressure(envoy={"digest": "", "tag": "v1.39.3"})
        values.update({"fullnameOverride": "custom-agent", "image": {"repository": "registry.example/agent", "digest": "sha256:" + "a" * 64},
                       "imagePullSecrets": [{"name": "registry-auth"}], "nodeSelector": {"kubernetes.io/arch": "arm64"},
                       "tolerations": [{"key": "dedicated", "operator": "Exists"}],
                       "affinity": {"nodeAffinity": {"preferredDuringSchedulingIgnoredDuringExecution": []}}})
        values["investigator"] = {"resources": {"requests": {"cpu": "100m", "memory": "128Mi"}}}
        documents = self.render(values)
        for workload in [item for item in documents if item["kind"] == "StatefulSet"]:
            pod = workload["spec"]["template"]["spec"]
            self.assertEqual(pod["imagePullSecrets"], values["imagePullSecrets"])
            self.assertEqual(pod["nodeSelector"], values["nodeSelector"])
            self.assertEqual(pod["tolerations"], values["tolerations"])
            self.assertEqual(pod["affinity"], values["affinity"])
            agent = pod["containers"][-1]
            self.assertEqual(agent["image"], "registry.example/agent@sha256:" + "a" * 64)
        collector = self.resource(documents, "StatefulSet", "custom-agent-collector")["spec"]["template"]["spec"]["containers"][0]
        self.assertEqual(collector["resources"]["requests"], {"cpu": "100m", "memory": "128Mi"})
        envoy = self.resource(documents, "StatefulSet", "custom-agent-backpressure")["spec"]["template"]["spec"]["containers"][0]
        self.assertEqual(envoy["image"], "envoyproxy/envoy:v1.39.3")

    def test_llm_secrets_are_referenced_not_embedded(self):
        documents = self.render({"investigator": {
            "auth": {"existingSecret": "shared-auth", "key": "bearer"},
            "llm": {"baseUrl": "http://llm.ai.svc:8080/v1", "model": "qwen-local",
                    "disableThinking": True, "existingSecret": "llm-auth", "key": "credential"},
        }})
        collector = self.resource(documents, "StatefulSet", "bug-agent-collector")["spec"]["template"]["spec"]["containers"][0]
        env = {item["name"]: item for item in collector["env"]}
        self.assertEqual(env["LLM_API_KEY"]["valueFrom"]["secretKeyRef"], {"name": "llm-auth", "key": "credential"})
        self.assertEqual(env["LLM_BASE_URL"]["value"], "http://llm.ai.svc:8080/v1")
        self.assertEqual(env["LLM_MODEL"]["value"], "qwen-local")
        self.assertEqual(env["AGENT_TOKEN"]["valueFrom"]["secretKeyRef"], {"name": "shared-auth", "key": "bearer"})
        self.assertIn("--disable-thinking", collector["args"])

    def test_http_only_collector_has_no_kubernetes_credentials(self):
        documents = self.render({"investigator": {"kubernetes": {"enabled": False}}})
        self.assertFalse(any(item["kind"] in {"Role", "RoleBinding"} for item in documents))
        pod = self.resource(documents, "StatefulSet", "bug-agent-collector")["spec"]["template"]["spec"]
        self.assertFalse(pod["automountServiceAccountToken"])
        self.assertNotIn("--namespace", pod["containers"][0]["args"])

    def test_invalid_values_fail_before_installation(self):
        invalid = [
            {"investigator": {"enabled": False}, "backpressure": {"enabled": False}},
            {"backpressure": {"enabled": True}},
            self.backpressure(cluster="bad.name"),
            self.backpressure(backend={"host": "http://backend"}),
            self.backpressure(backend={"host": "bug-agent-backpressure.demo.svc"}),
            self.backpressure(backend={"host": "bug-agent-backpressure-headless"}),
            self.backpressure(policy={"initialConcurrency": 20}),
            self.backpressure(policy={"minConcurrency": 9}),
            self.backpressure(policy={"maxConcurrency": 0}),
            self.backpressure(policy={"errorRatio": 1.1}),
            self.backpressure(policy={"latencyMs": -1}),
            self.backpressure(policy={"pollSeconds": 61}),
            self.backpressure(policy={"healthyWindows": 0}),
            self.backpressure(policy={"minSamples": 0}),
            self.backpressure(envoy={"routeTimeout": "bad"}),
            {"investigator": {"replicas": 2}},
            {"investigator": {"auth": {"existingSecret": ""}}},
            {"investigator": {"llm": {"existingSecret": "llm-auth"}}},
            {"image": {"digest": "not-a-digest"}},
            {"fullnameOverride": "INVALID"},
        ]
        for values in invalid:
            with self.subTest(values=values):
                result = subprocess.run(
                    ["helm", "template", "bug-agent", str(CHART), "--namespace", "demo", "--values", "-"],
                    input=yaml.safe_dump(values), text=True, capture_output=True, timeout=30,
                )
                self.assertNotEqual(result.returncode, 0)
        old_cluster = subprocess.run(
            ["helm", "template", "bug-agent", str(CHART), "--kube-version", "1.32.0"],
            text=True, capture_output=True, timeout=30,
        )
        self.assertNotEqual(old_cluster.returncode, 0)

    def test_long_release_names_are_bounded_and_do_not_collide(self):
        names = []
        for release in ["a" * 50 + "one", "a" * 50 + "two"]:
            documents = self.render(self.backpressure(), release=release)
            self.assertTrue(all(len(item["metadata"]["name"]) <= 63 for item in documents))
            names.append({item["metadata"]["name"] for item in documents})
        self.assertFalse(names[0] & names[1])

    def test_packaged_chart_contains_its_bootstrap_and_schema(self):
        scratch = ROOT / "tmp"
        scratch.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch) as directory:
            result = subprocess.run(["helm", "package", str(CHART), "--destination", directory],
                                    text=True, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            archive = Path(directory) / "kube-bug-agent-0.1.0.tgz"
            with tarfile.open(archive) as package:
                names = package.getnames()
                self.assertIn("kube-bug-agent/files/envoy.json", names)
                self.assertIn("kube-bug-agent/values.schema.json", names)
                self.assertIn("kube-bug-agent/README.md", names)
        packaged_base = json.loads((CHART / "files" / "envoy.json").read_text())
        deployment_base = json.loads((ROOT / "deploy" / "backpressure" / "envoy-kubernetes.json").read_text())
        self.assertEqual(packaged_base, deployment_base)

    @unittest.skipUnless(os.environ.get("HELM_ENVOY_SMOKE"), "run make smoke-helm to validate with real Envoy")
    def test_envoy_bootstrap_is_validated_by_real_envoy(self):
        documents = self.render(self.backpressure(cluster="payments", backend={"host": "payments-backend", "port": 9000}))
        scratch = ROOT / "tmp"
        scratch.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch) as directory:
            Path(directory).chmod(0o755)
            config_path = Path(directory) / "envoy.json"
            config_path.write_text(json.dumps(self.envoy(documents)))
            envoy = self.resource(documents, "StatefulSet", "bug-agent-backpressure")["spec"]["template"]["spec"]["containers"][0]
            result = subprocess.run(
                ["docker", "run", "--rm", "--user", "10001:10001", "--entrypoint", "envoy",
                 "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges", "--tmpfs", "/tmp",
                 "--mount", f"type=bind,source={config_path},target=/etc/envoy/envoy.json,readonly",
                 envoy["image"], "-c", "/etc/envoy/envoy.json", "--mode", "validate",
                 "--concurrency", "1", "--disable-hot-restart"],
                text=True, capture_output=True, timeout=120,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
