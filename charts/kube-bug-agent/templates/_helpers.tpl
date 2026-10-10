{{- define "kube-bug-agent.fullname" -}}
{{- $name := default .Release.Name .Values.fullnameOverride -}}
{{- if gt (len $name) 41 -}}
{{- printf "%s-%s" ($name | trunc 32 | trimSuffix "-") ($name | sha256sum | trunc 8) -}}
{{- else -}}
{{- $name -}}
{{- end -}}
{{- end -}}

{{- define "kube-bug-agent.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | quote }}
app.kubernetes.io/name: {{ .Chart.Name | quote }}
app.kubernetes.io/instance: {{ .Release.Name | quote }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
{{- end -}}

{{- define "kube-bug-agent.selectorLabels" -}}
app.kubernetes.io/name: {{ .root.Chart.Name | quote }}
app.kubernetes.io/instance: {{ .root.Release.Name | quote }}
app.kubernetes.io/component: {{ .component | quote }}
{{- end -}}

{{- define "kube-bug-agent.image" -}}
{{- if .digest -}}
{{- printf "%s@%s" .repository .digest -}}
{{- else -}}
{{- printf "%s:%s" .repository .tag -}}
{{- end -}}
{{- end -}}

{{- define "kube-bug-agent.podSecurity" -}}
runAsNonRoot: true
runAsUser: 10001
runAsGroup: 10001
fsGroup: 10001
seccompProfile:
  type: RuntimeDefault
{{- end -}}

{{- define "kube-bug-agent.containerSecurity" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
capabilities:
  drop: [ALL]
{{- end -}}

{{- define "kube-bug-agent.scheduling" -}}
{{- with .Values.imagePullSecrets }}
imagePullSecrets:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- with .Values.nodeSelector }}
nodeSelector:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- with .Values.tolerations }}
tolerations:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- with .Values.affinity }}
affinity:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- end -}}

{{- define "kube-bug-agent.claim" -}}
{{- if eq .storageClass "-" }}
storageClassName: ""
{{- else if .storageClass }}
storageClassName: {{ .storageClass | quote }}
{{- end }}
accessModes: [ReadWriteOnce]
resources:
  requests:
    storage: {{ .size | quote }}
{{- end -}}
