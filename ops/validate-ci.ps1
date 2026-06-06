param(
    [switch]$SkipFrontend,
    [switch]$SkipDocker,
    [switch]$StrictExternalTools
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

$localToolBin = Join-Path $repoRoot ".tools\bin"
if (Test-Path -LiteralPath $localToolBin) {
    $env:PATH = "$localToolBin;$env:PATH"
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action
    )

    Write-Host "==> $Name"
    & $Action
    Write-Host ""
}

function Test-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Warn-Or-Fail-MissingTool {
    param([Parameter(Mandatory = $true)][string]$Name)

    if ($StrictExternalTools) {
        throw "$Name is required when -StrictExternalTools is set"
    }

    Write-Warning "$Name is not installed; skipping semantic validation that requires it"
}

Invoke-Step "OpenAPI YAML, refs, and typed route guard" {
    $script = @'
import pathlib
import yaml

spec_path = pathlib.Path("backend/openapi.yaml")
text = spec_path.read_text(encoding="utf-8")
spec = yaml.safe_load(text)

missing = []

def walk(value):
    if isinstance(value, dict):
        ref = value.get("$ref")
        if isinstance(ref, str) and ref.startswith("#/"):
            node = spec
            for part in ref[2:].split("/"):
                node = node.get(part) if isinstance(node, dict) else None
            if node is None:
                missing.append(ref)
        for child in value.values():
            walk(child)
    elif isinstance(value, list):
        for child in value:
            walk(child)

walk(spec)
if missing:
    print("missing_refs", sorted(set(missing)))
    raise SystemExit(1)

for forbidden in (
    "#/components/responses/ApiSuccess",
    "#/components/requestBodies/JsonObject",
):
    if forbidden in text:
        raise SystemExit(f"forbidden generic OpenAPI reference remains: {forbidden}")

print("openapi ok")
'@
    $script | python -
}

Invoke-Step "YAML and JSON assets" {
    $script = @'
import json
import pathlib
import yaml

yaml_paths = [
    pathlib.Path("compose.yaml"),
    pathlib.Path(".github/workflows/ci.yml"),
    pathlib.Path("backend/openapi.yaml"),
    pathlib.Path("ops/prometheus/prometheus.yml"),
    pathlib.Path("ops/prometheus/alerts/backend.yml"),
    pathlib.Path("ops/alertmanager/alertmanager.yml"),
    pathlib.Path("ops/alertmanager/alertmanager.webhook.example.yml"),
    pathlib.Path("ops/alertmanager/alertmanager.email.example.yml"),
    pathlib.Path("ops/alertmanager/alertmanager.production.example.yml"),
    pathlib.Path("ops/grafana/provisioning/datasources/prometheus.yml"),
    pathlib.Path("ops/grafana/provisioning/dashboards/dashboards.yml"),
]

for path in yaml_paths:
    yaml.safe_load(path.read_text(encoding="utf-8"))

for path in pathlib.Path("ops/grafana/dashboards").glob("*.json"):
    json.loads(path.read_text(encoding="utf-8"))

print("yaml/json assets ok")
'@
    $script | python -
}

Invoke-Step "Backend tests" {
    cargo test --manifest-path backend/Cargo.toml
}

Invoke-Step "Client SDK tests" {
    cargo test --manifest-path client-sdk/Cargo.toml
}

if (-not $SkipFrontend) {
    Invoke-Step "Admin lint" {
        npm -C admin run lint
    }

    Invoke-Step "Admin build" {
        npm -C admin run build
    }
}

if (-not $SkipDocker) {
    if (Test-Command "docker") {
        Invoke-Step "Docker Compose config" {
            $previousComposeEnvFile = $env:COMPOSE_ENV_FILE
            try {
                $env:COMPOSE_ENV_FILE = ".env.compose.example"
                docker compose -p user-admin --env-file .env.compose.example config
            } finally {
                $env:COMPOSE_ENV_FILE = $previousComposeEnvFile
            }
        }

        Invoke-Step "Docker Compose image pins" {
            & (Join-Path $repoRoot "ops/check-compose-image-pins.ps1") -EnvFile ".env.compose.example"
        }
    } else {
        Warn-Or-Fail-MissingTool "docker"
    }
}

if (Test-Command "promtool") {
    Invoke-Step "Prometheus config and rules" {
        promtool check config ops/prometheus/prometheus.yml
        promtool check rules ops/prometheus/alerts/backend.yml
    }
} else {
    Warn-Or-Fail-MissingTool "promtool"
}

if (Test-Command "amtool") {
    Invoke-Step "Alertmanager configs" {
        Get-ChildItem -LiteralPath "ops/alertmanager" -Filter "*.yml" |
            ForEach-Object { amtool check-config $_.FullName }
    }
} else {
    Warn-Or-Fail-MissingTool "amtool"
}

Write-Host "All enabled checks passed."
