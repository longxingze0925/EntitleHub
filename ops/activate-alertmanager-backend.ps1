#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$OutputPath = ".tools/alertmanager.backend.yml",
    [string]$BackendWebhookUrl = "http://backend:8080/api/internal/alertmanager/webhook",
    [string]$ProjectName = "user-admin",
    [switch]$Check,
    [switch]$Restart
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

function New-UrlSafeSecret {
    return [Convert]::ToBase64String((New-RandomBytes 32)).TrimEnd("=").Replace("+", "-").Replace("/", "_")
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

$lines = @(Get-Content -LiteralPath $envPath -Encoding UTF8)
$values = Read-EnvFile -Path $envPath

$token = ""
if ($values.ContainsKey("ALERTMANAGER_WEBHOOK_TOKEN")) {
    $token = $values["ALERTMANAGER_WEBHOOK_TOKEN"].Trim()
}
if ($token.Length -lt 32) {
    $token = New-UrlSafeSecret
    $lines = @(Set-EnvValue -Lines $lines -Key "ALERTMANAGER_WEBHOOK_TOKEN" -Value $token)
    Write-Host "Generated ALERTMANAGER_WEBHOOK_TOKEN in $envPath"
}

$lines = @(Set-EnvValue -Lines $lines -Key "ALERTMANAGER_CONFIG_PATH" -Value $OutputPath)
Set-Content -LiteralPath $envPath -Value $lines -Encoding UTF8

$renderScript = Resolve-RepoPath "ops/render-alertmanager-config.ps1"
$renderArgs = @{
    Mode = "backend"
    OutputPath = $OutputPath
    BackendWebhookUrl = $BackendWebhookUrl
    AlertmanagerWebhookToken = $token
}
if ($Check) {
    $renderArgs["Check"] = $true
}

& $renderScript @renderArgs

Write-Host "Configured ALERTMANAGER_CONFIG_PATH=$OutputPath in $envPath"

if ($Restart) {
    Invoke-NativeCommandWithRetry -Name "docker compose alertmanager restart" -Command {
        & docker compose -p $ProjectName --env-file $envPath up -d --force-recreate alertmanager
    }
}
