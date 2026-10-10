{{- define "kube-bug-agent.envoy" -}}
{{- $config := .Files.Get "files/envoy.json" | fromJson -}}
{{- $bp := .Values.backpressure -}}
{{- $cluster := index $config.static_resources.clusters 0 -}}
{{- $_ := set $cluster "name" $bp.cluster -}}
{{- $_ := set $cluster "connect_timeout" $bp.envoy.connectTimeout -}}
{{- $_ := set $cluster.load_assignment "cluster_name" $bp.cluster -}}
{{- $endpoint := (index (index $cluster.load_assignment.endpoints 0).lb_endpoints 0).endpoint.address.socket_address -}}
{{- $_ := set $endpoint "address" $bp.backend.host -}}
{{- $_ := set $endpoint "port_value" $bp.backend.port -}}
{{- $thresholds := index $cluster.circuit_breakers.thresholds 0 -}}
{{- $_ := set $thresholds "max_requests" $bp.policy.initialConcurrency -}}
{{- $_ := set $thresholds "max_pending_requests" $bp.envoy.maxPendingRequests -}}
{{- $_ := set $thresholds "max_connections" $bp.envoy.maxConnections -}}
{{- $static := dict (printf "circuit_breakers.%s.default.max_requests" $bp.cluster) $bp.policy.initialConcurrency "envoy.reloadable_features.skip_pending_overflow_count_on_active_rq" false -}}
{{- $_ := set (index $config.layered_runtime.layers 0) "static_layer" $static -}}
{{- $listener := index $config.static_resources.listeners 0 -}}
{{- $_ := set $listener "name" (printf "protected_%s" $bp.cluster) -}}
{{- $manager := (index (index $listener.filter_chains 0).filters 0).typed_config -}}
{{- $_ := set $manager "stat_prefix" (printf "protected_%s" $bp.cluster) -}}
{{- $_ := set $manager "request_timeout" $bp.envoy.requestTimeout -}}
{{- $_ := set $manager "stream_idle_timeout" $bp.envoy.streamIdleTimeout -}}
{{- $_ := set $manager.route_config "name" $bp.cluster -}}
{{- $virtual := index $manager.route_config.virtual_hosts 0 -}}
{{- $_ := set $virtual "name" $bp.cluster -}}
{{- $route := (index $virtual.routes 0).route -}}
{{- $_ := set $route "cluster" $bp.cluster -}}
{{- $_ := set $route "timeout" $bp.envoy.routeTimeout -}}
{{- $config | toPrettyJson -}}
{{- end -}}
