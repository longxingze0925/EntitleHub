#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$ProjectName = "user-admin",
    [string]$HostName = "localhost",
    [int]$TimeoutSeconds = 10,
    [int]$WaitSeconds = 40,
    [switch]$AllowConfiguredChannels,
    [switch]$SkipChannelCheck
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

function Invoke-NativeCommandWithRetry {
    param(
        [string]$Name,
        [scriptblock]$Command,
        [int]$Attempts = 3
    )

    $lastError = ""
    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        try {
            $output = & $Command
            if ($LASTEXITCODE -eq 0) {
                return $output
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

function Get-MetricValue {
    param(
        [string]$MetricsText,
        [string]$Name,
        [string]$Integration = "webhook"
    )

    $escapedName = [regex]::Escape($Name)
    $escapedIntegration = [regex]::Escape($Integration)
    $pattern = "(?m)^$escapedName\{integration=`"$escapedIntegration`"\}\s+([0-9]+(?:\.[0-9]+)?)$"
    $match = [regex]::Match($MetricsText, $pattern)
    if (-not $match.Success) {
        return 0.0
    }

    return [double]::Parse($match.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
}

function Invoke-AlertmanagerGet {
    param([string]$Path)

    Invoke-RestMethod -Uri "$alertmanagerUrl$Path" -TimeoutSec $TimeoutSeconds
}

function Invoke-AlertmanagerPostAlerts {
    param([string]$Body)

    Invoke-RestMethod -Uri "$alertmanagerUrl/api/v2/alerts" -Method Post -ContentType "application/json" -Body $Body -TimeoutSec $TimeoutSeconds | Out-Null
}

function New-SmokeAlertJson {
    param(
        [datetime]$StartsAt,
        [datetime]$EndsAt
    )

    $starts = $StartsAt.ToUniversalTime().ToString("o")
    $ends = $EndsAt.ToUniversalTime().ToString("o")
    return "[{`"labels`":{`"alertname`":`"CodexSmokeAlert`",`"severity`":`"info`",`"instance`":`"codex-smoke`"},`"annotations`":{`"summary`":`"Codex smoke alert`",`"description`":`"Synthetic Alertmanager backend adapter test`"},`"startsAt`":`"$starts`",`"endsAt`":`"$ends`",`"generatorURL`":`"http://localhost/codex-smoke`"}]"
}

$envPath = Resolve-RepoPath $EnvFile
if (-not (Test-Path -LiteralPath $envPath)) {
    throw "Env file not found: $envPath"
}

$envValues = Read-EnvFile -Path $envPath
$alertmanagerPort = Get-PortValue -Values $envValues -Key "ALERTMANAGER_HOST_PORT" -Default 9093
$alertmanagerUrl = "http://${HostName}:$alertmanagerPort"

Write-Host "==> Alertmanager backend receiver check"
$status = Invoke-AlertmanagerGet -Path "/api/v2/status"
$configText = [string]$status.config.original
if ($configText -notlike "*backend-notification-channels*") {
    throw "Alertmanager is not using the backend notification receiver. Run ops/activate-alertmanager-backend.ps1 -Check -Restart first."
}
Write-Host "Alertmanager backend receiver is active."

if (-not $SkipChannelCheck) {
    Write-Host "==> Notification channel safety check"
    $sql = "select count(*) from notification_channels where enabled = true and secret_encrypted is not null;"
    $countText = Invoke-NativeCommandWithRetry -Name "notification channel count query" -Command {
        & docker compose -p $ProjectName --env-file $envPath exec -T postgres psql -U app_user -d user_admin -tAc $sql
    }
    $channelCount = [int]($countText | Select-Object -First 1).Trim()
    Write-Host "Enabled configured notification channels: $channelCount"
    if ($channelCount -gt 0 -and -not $AllowConfiguredChannels) {
        throw "Refusing to send a smoke alert because configured notification channels exist. Re-run with -AllowConfiguredChannels only when real outbound notifications are expected."
    }
}

Write-Host "==> Alertmanager webhook metric baseline"
$metricsBefore = (Invoke-WebRequest -Uri "$alertmanagerUrl/metrics" -UseBasicParsing -TimeoutSec $TimeoutSeconds).Content
$requestsBefore = Get-MetricValue -MetricsText $metricsBefore -Name "alertmanager_notification_requests_total"
$notificationsBefore = Get-MetricValue -MetricsText $metricsBefore -Name "alertmanager_notifications_total"
$errorsBefore = Get-MetricValue -MetricsText $metricsBefore -Name "alertmanager_notification_errors_total"

$submitted = $false
try {
    Write-Host "==> Submit synthetic alert"
    $now = Get-Date
    Invoke-AlertmanagerPostAlerts -Body (New-SmokeAlertJson -StartsAt $now -EndsAt $now.AddMinutes(5))
    $submitted = $true

    Write-Host "Waiting $WaitSeconds seconds for Alertmanager group_wait and webhook delivery..."
    Start-Sleep -Seconds $WaitSeconds

    $metricsAfter = (Invoke-WebRequest -Uri "$alertmanagerUrl/metrics" -UseBasicParsing -TimeoutSec $TimeoutSeconds).Content
    $requestsAfter = Get-MetricValue -MetricsText $metricsAfter -Name "alertmanager_notification_requests_total"
    $notificationsAfter = Get-MetricValue -MetricsText $metricsAfter -Name "alertmanager_notifications_total"
    $errorsAfter = Get-MetricValue -MetricsText $metricsAfter -Name "alertmanager_notification_errors_total"

    $result = [pscustomobject]@{
        RequestsBefore = $requestsBefore
        RequestsAfter = $requestsAfter
        NotificationsBefore = $notificationsBefore
        NotificationsAfter = $notificationsAfter
        ErrorsBefore = $errorsBefore
        ErrorsAfter = $errorsAfter
    }
    $result | Format-Table -AutoSize

    if ($requestsAfter -le $requestsBefore) {
        throw "Alertmanager webhook request counter did not increase."
    }
    if ($notificationsAfter -le $notificationsBefore) {
        throw "Alertmanager webhook notification counter did not increase."
    }
    if ($errorsAfter -gt $errorsBefore) {
        throw "Alertmanager webhook error counter increased."
    }
} finally {
    if ($submitted) {
        Write-Host "==> Resolve synthetic alert"
        $resolvedAt = (Get-Date).AddSeconds(-1)
        Invoke-AlertmanagerPostAlerts -Body (New-SmokeAlertJson -StartsAt $resolvedAt.AddMinutes(-5) -EndsAt $resolvedAt)
        Start-Sleep -Seconds 5
    }
}

$activeSmokeAlerts = @((Invoke-AlertmanagerGet -Path "/api/v2/alerts") | Where-Object { $_.labels.alertname -eq "CodexSmokeAlert" })
if ($activeSmokeAlerts.Count -gt 0) {
    throw "Synthetic smoke alert is still active."
}

Write-Host "Alertmanager backend smoke passed."
