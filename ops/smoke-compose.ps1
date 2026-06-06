#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$ProjectName = "user-admin",
    [string]$HostName = "localhost",
    [int]$TimeoutSeconds = 10,
    [switch]$SkipDockerPs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$repoRoot = Split-Path -Parent $PSScriptRoot

function Resolve-RepoPath {
    param([string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }

    return (Join-Path $repoRoot $Path)
}

function Read-EnvFile {
    param([string]$Path)

    $values = @{}
    foreach ($rawLine in Get-Content -LiteralPath $Path -Encoding UTF8) {
        $line = $rawLine.Trim()
        if ($line.Length -eq 0 -or $line.StartsWith("#")) {
            continue
        }

        $separator = $line.IndexOf("=")
        if ($separator -lt 1) {
            continue
        }

        $key = $line.Substring(0, $separator).Trim()
        $value = $line.Substring($separator + 1).Trim()
        if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
            $value = $value.Substring(1, $value.Length - 2)
        }

        $values[$key] = $value
    }

    return $values
}

function Get-EnvValue {
    param(
        [hashtable]$Values,
        [string]$Key,
        [string]$Default
    )

    if ($Values.ContainsKey($Key) -and -not [string]::IsNullOrWhiteSpace($Values[$Key])) {
        return $Values[$Key]
    }

    return $Default
}

function Get-PortValue {
    param(
        [hashtable]$Values,
        [string]$Key,
        [int]$Default
    )

    $raw = Get-EnvValue -Values $Values -Key $Key -Default ([string]$Default)
    $port = 0
    if (-not [int]::TryParse($raw, [ref]$port) -or $port -lt 1 -or $port -gt 65535) {
        throw "$Key must be a valid TCP port."
    }

    return $port
}

function Invoke-HttpCheck {
    param(
        [string]$Name,
        [string]$Url,
        [int]$TimeoutSeconds,
        [string]$Contains
    )

    try {
        $response = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec $TimeoutSeconds
        if ($response.StatusCode -lt 200 -or $response.StatusCode -gt 299) {
            return [pscustomobject]@{ Name = $Name; Status = "FAIL"; Detail = "HTTP $($response.StatusCode)" }
        }

        if ($Contains -and $response.Content -notlike "*$Contains*") {
            return [pscustomobject]@{ Name = $Name; Status = "FAIL"; Detail = "missing content: $Contains" }
        }

        return [pscustomobject]@{ Name = $Name; Status = "PASS"; Detail = "$Url -> HTTP $($response.StatusCode)" }
    } catch {
        return [pscustomobject]@{ Name = $Name; Status = "FAIL"; Detail = $_.Exception.Message }
    }
}

function Invoke-PrometheusTargetCheck {
    param(
        [string]$Url,
        [int]$TimeoutSeconds
    )

    try {
        $payload = Invoke-RestMethod -Uri $Url -TimeoutSec $TimeoutSeconds
        $downTargets = @($payload.data.activeTargets | Where-Object { $_.health -ne "up" })
        if ($downTargets.Count -gt 0) {
            $details = ($downTargets | ForEach-Object { "$($_.labels.job)=$($_.health)" }) -join ", "
            return [pscustomobject]@{ Name = "prometheus targets"; Status = "FAIL"; Detail = $details }
        }

        $targetCount = @($payload.data.activeTargets).Count
        return [pscustomobject]@{ Name = "prometheus targets"; Status = "PASS"; Detail = "$targetCount active targets up" }
    } catch {
        return [pscustomobject]@{ Name = "prometheus targets"; Status = "FAIL"; Detail = $_.Exception.Message }
    }
}

function Invoke-NativeCommandWithRetry {
    param(
        [string]$Name,
        [scriptblock]$Command,
        [int]$Attempts = 3
    )

    $lastError = ""
    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        try {
            & $Command
            if ($LASTEXITCODE -eq 0) {
                return
            }
            $lastError = "exit code $LASTEXITCODE"
        } catch {
            $lastError = $_.Exception.Message
        }

        if ($attempt -lt $Attempts) {
            Write-Warning "$Name failed on attempt $attempt/${Attempts}: $lastError. Retrying..."
            Start-Sleep -Seconds ([Math]::Min(2 * $attempt, 10))
        }
    }

    throw "$Name failed after $Attempts attempts: $lastError"
}

$envPath = Resolve-RepoPath $EnvFile
if (-not (Test-Path -LiteralPath $envPath)) {
    throw "Env file not found: $envPath"
}

$envValues = Read-EnvFile -Path $envPath

$adminPort = Get-PortValue -Values $envValues -Key "ADMIN_HOST_PORT" -Default 5173
$backendPort = Get-PortValue -Values $envValues -Key "BACKEND_HOST_PORT" -Default 8080
$prometheusPort = Get-PortValue -Values $envValues -Key "PROMETHEUS_HOST_PORT" -Default 9090
$alertmanagerPort = Get-PortValue -Values $envValues -Key "ALERTMANAGER_HOST_PORT" -Default 9093
$grafanaPort = Get-PortValue -Values $envValues -Key "GRAFANA_HOST_PORT" -Default 3000

if (-not $SkipDockerPs) {
    Write-Host "==> Docker Compose status"
    Invoke-NativeCommandWithRetry -Name "docker compose ps" -Command {
        & docker compose -p $ProjectName --env-file $envPath ps
    }
}

Write-Host "==> HTTP smoke checks"
$checks = @(
    @{ Name = "backend health"; Url = "http://${HostName}:$backendPort/health"; Contains = '"status":"ok"' },
    @{ Name = "backend ready"; Url = "http://${HostName}:$backendPort/readyz"; Contains = '"status":"ok"' },
    @{ Name = "backend metrics"; Url = "http://${HostName}:$backendPort/metrics"; Contains = "http_requests_total" },
    @{ Name = "admin"; Url = "http://${HostName}:$adminPort/"; Contains = "<html" },
    @{ Name = "prometheus ready"; Url = "http://${HostName}:$prometheusPort/-/ready"; Contains = "Prometheus Server is Ready" },
    @{ Name = "alertmanager ready"; Url = "http://${HostName}:$alertmanagerPort/-/ready"; Contains = "" },
    @{ Name = "grafana health"; Url = "http://${HostName}:$grafanaPort/api/health"; Contains = '"database"' }
)

$results = foreach ($check in $checks) {
    Invoke-HttpCheck -Name $check.Name -Url $check.Url -TimeoutSeconds $TimeoutSeconds -Contains $check.Contains
}

$results += Invoke-PrometheusTargetCheck -Url "http://${HostName}:$prometheusPort/api/v1/targets?state=active" -TimeoutSeconds $TimeoutSeconds
$results | Format-Table -AutoSize

$failed = @($results | Where-Object { $_.Status -ne "PASS" })
if ($failed.Count -gt 0) {
    exit 1
}

Write-Host "All smoke checks passed."
