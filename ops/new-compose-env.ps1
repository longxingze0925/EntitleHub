#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$TemplatePath = ".env.compose.example",
    [string]$OutputPath = ".env.compose",
    [switch]$Force,
    [string]$HostBind = "127.0.0.1",
    [int]$PostgresHostPort = 5432,
    [int]$RedisHostPort = 6379,
    [int]$PostgresExporterHostPort = 9187,
    [int]$RedisExporterHostPort = 9121,
    [int]$BackendHostPort = 8080,
    [int]$AdminHostPort = 5173,
    [int]$PrometheusHostPort = 9090,
    [int]$AlertmanagerHostPort = 9093,
    [int]$GrafanaHostPort = 3000
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot

function Resolve-RepoPath {
    param([string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }

    return (Join-Path $repoRoot $Path)
}

function Assert-Port {
    param(
        [string]$Name,
        [int]$Port
    )

    if ($Port -lt 1 -or $Port -gt 65535) {
        throw "$Name must be between 1 and 65535."
    }
}

function New-RandomBytes {
    param([int]$Length)

    $bytes = New-Object byte[] $Length
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }
    return $bytes
}

function New-Base64Key {
    return [Convert]::ToBase64String((New-RandomBytes 32))
}

function New-UrlSafeSecret {
    return [Convert]::ToBase64String((New-RandomBytes 32)).TrimEnd("=").Replace("+", "-").Replace("/", "_")
}

function Get-EnvValue {
    param(
        [string[]]$Lines,
        [string]$Key,
        [string]$Default
    )

    $pattern = "^$([regex]::Escape($Key))=(.*)$"
    foreach ($line in $Lines) {
        $match = [regex]::Match($line, $pattern)
        if ($match.Success) {
            return $match.Groups[1].Value
        }
    }

    return $Default
}

function Set-EnvValue {
    param(
        [string[]]$Lines,
        [string]$Key,
        [string]$Value
    )

    $pattern = "^$([regex]::Escape($Key))="
    $updated = $false
    for ($i = 0; $i -lt $Lines.Count; $i++) {
        if ($Lines[$i] -match $pattern) {
            $Lines[$i] = "$Key=$Value"
            $updated = $true
            break
        }
    }

    if (-not $updated) {
        $Lines = @($Lines + "$Key=$Value")
    }

    return $Lines
}

$parsedHost = $null
if (-not [System.Net.IPAddress]::TryParse($HostBind, [ref]$parsedHost)) {
    throw "HostBind must be a valid IP address."
}

foreach ($item in @(
    @{ Name = "PostgresHostPort"; Port = $PostgresHostPort },
    @{ Name = "RedisHostPort"; Port = $RedisHostPort },
    @{ Name = "PostgresExporterHostPort"; Port = $PostgresExporterHostPort },
    @{ Name = "RedisExporterHostPort"; Port = $RedisExporterHostPort },
    @{ Name = "BackendHostPort"; Port = $BackendHostPort },
    @{ Name = "AdminHostPort"; Port = $AdminHostPort },
    @{ Name = "PrometheusHostPort"; Port = $PrometheusHostPort },
    @{ Name = "AlertmanagerHostPort"; Port = $AlertmanagerHostPort },
    @{ Name = "GrafanaHostPort"; Port = $GrafanaHostPort }
)) {
    Assert-Port -Name $item.Name -Port $item.Port
}

$template = Resolve-RepoPath $TemplatePath
$output = Resolve-RepoPath $OutputPath

if (-not (Test-Path -LiteralPath $template)) {
    throw "Template not found: $template"
}

if ((Test-Path -LiteralPath $output) -and -not $Force) {
    throw "Output already exists: $output. Re-run with -Force to overwrite it."
}

$lines = @(Get-Content -LiteralPath $template -Encoding UTF8)

$postgresDb = Get-EnvValue -Lines $lines -Key "POSTGRES_DB" -Default "user_admin"
$postgresUser = Get-EnvValue -Lines $lines -Key "POSTGRES_USER" -Default "app_user"
$postgresPassword = New-UrlSafeSecret
$redisPassword = New-UrlSafeSecret
$grafanaPassword = New-UrlSafeSecret

$values = [ordered]@{
    COMPOSE_ENV_FILE = $OutputPath
    POSTGRES_PASSWORD = $postgresPassword
    REDIS_PASSWORD = $redisPassword
    GRAFANA_ADMIN_USER = "admin"
    GRAFANA_ADMIN_PASSWORD = $grafanaPassword
    COMPOSE_HOST_BIND = $HostBind
    POSTGRES_HOST_PORT = [string]$PostgresHostPort
    REDIS_HOST_PORT = [string]$RedisHostPort
    POSTGRES_EXPORTER_HOST_PORT = [string]$PostgresExporterHostPort
    REDIS_EXPORTER_HOST_PORT = [string]$RedisExporterHostPort
    BACKEND_HOST_PORT = [string]$BackendHostPort
    ADMIN_HOST_PORT = [string]$AdminHostPort
    PROMETHEUS_HOST_PORT = [string]$PrometheusHostPort
    ALERTMANAGER_HOST_PORT = [string]$AlertmanagerHostPort
    GRAFANA_HOST_PORT = [string]$GrafanaHostPort
    ALERTMANAGER_CONFIG_PATH = "./ops/alertmanager/alertmanager.yml"
    APP_BASE_URL = "http://localhost:$AdminHostPort"
    ALLOWED_ORIGINS = "http://localhost:$AdminHostPort,http://127.0.0.1:$AdminHostPort"
    DATABASE_URL = "postgres://${postgresUser}:${postgresPassword}@postgres:5432/${postgresDb}"
    REDIS_URL = "redis://:${redisPassword}@redis:6379"
    SESSION_SECRET = New-UrlSafeSecret
    TOKEN_HASH_PEPPER = New-UrlSafeSecret
    REFRESH_TOKEN_PEPPER = New-UrlSafeSecret
    CSRF_SECRET = New-UrlSafeSecret
    MASTER_KEY = New-Base64Key
    ALERTMANAGER_WEBHOOK_TOKEN = New-UrlSafeSecret
    JWT_ISSUER = "http://localhost:$BackendHostPort"
}

foreach ($entry in $values.GetEnumerator()) {
    $lines = @(Set-EnvValue -Lines $lines -Key $entry.Key -Value $entry.Value)
}

Set-Content -LiteralPath $output -Value $lines -Encoding UTF8

Write-Host "Wrote $output"
Write-Host "Start the stack with:"
Write-Host "docker compose -p user-admin --env-file $OutputPath up -d --build"
