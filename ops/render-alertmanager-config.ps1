#requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet("noop", "backend", "webhook", "email", "production")]
    [string]$Mode = "noop",
    [string]$OutputPath = ".tools/alertmanager.generated.yml",
    [string]$BackendWebhookUrl = $env:ALERTMANAGER_BACKEND_WEBHOOK_URL,
    [string]$AlertmanagerWebhookToken = $env:ALERTMANAGER_WEBHOOK_TOKEN,
    [string]$WebhookUrl = $env:ALERTMANAGER_WEBHOOK_URL,
    [string]$CriticalWebhookUrl = $env:ALERTMANAGER_CRITICAL_WEBHOOK_URL,
    [string]$EmailTo = $env:ALERTMANAGER_EMAIL_TO,
    [string]$CriticalEmailTo = $env:ALERTMANAGER_CRITICAL_EMAIL_TO,
    [string]$WarningEmailTo = $env:ALERTMANAGER_WARNING_EMAIL_TO,
    [string]$SmtpSmarthost = $env:ALERTMANAGER_SMTP_SMARTHOST,
    [string]$SmtpFrom = $env:ALERTMANAGER_SMTP_FROM,
    [string]$SmtpUsername = $env:ALERTMANAGER_SMTP_USERNAME,
    [string]$SmtpPassword = $env:ALERTMANAGER_SMTP_PASSWORD,
    [bool]$SmtpRequireTls = $true,
    [switch]$Check
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$localToolBin = Join-Path $repoRoot ".tools\bin"
if (Test-Path -LiteralPath $localToolBin) {
    $env:PATH = "$localToolBin;$env:PATH"
}

function Resolve-RepoPath {
    param([string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }

    return (Join-Path $repoRoot $Path)
}

function Get-RequiredValue {
    param(
        [string]$Name,
        [string]$Value
    )

    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "$Name is required for Alertmanager $Mode mode."
    }

    return $Value.Trim()
}

function ConvertTo-YamlScalar {
    param([string]$Value)

    return "'" + $Value.Replace("'", "''") + "'"
}

function Get-Smarthost {
    if (-not [string]::IsNullOrWhiteSpace($SmtpSmarthost)) {
        return $SmtpSmarthost.Trim()
    }

    if (-not [string]::IsNullOrWhiteSpace($env:SMTP_HOST)) {
        $port = if ([string]::IsNullOrWhiteSpace($env:SMTP_PORT)) { "587" } else { $env:SMTP_PORT.Trim() }
        return "$($env:SMTP_HOST.Trim()):$port"
    }

    return ""
}

function Get-SmtpBlock {
    $smarthost = Get-RequiredValue -Name "ALERTMANAGER_SMTP_SMARTHOST" -Value (Get-Smarthost)
    $from = Get-RequiredValue -Name "ALERTMANAGER_SMTP_FROM" -Value $(if ($SmtpFrom) { $SmtpFrom } else { $env:SMTP_FROM })
    $username = Get-RequiredValue -Name "ALERTMANAGER_SMTP_USERNAME" -Value $(if ($SmtpUsername) { $SmtpUsername } else { $env:SMTP_USER })
    $password = Get-RequiredValue -Name "ALERTMANAGER_SMTP_PASSWORD" -Value $(if ($SmtpPassword) { $SmtpPassword } else { $env:SMTP_PASSWORD })
    $tls = $SmtpRequireTls.ToString().ToLowerInvariant()

    return @"
global:
  resolve_timeout: 5m
  smtp_smarthost: $(ConvertTo-YamlScalar $smarthost)
  smtp_from: $(ConvertTo-YamlScalar $from)
  smtp_auth_username: $(ConvertTo-YamlScalar $username)
  smtp_auth_password: $(ConvertTo-YamlScalar $password)
  smtp_require_tls: $tls
"@
}

function New-NoopConfig {
    return @"
global:
  resolve_timeout: 5m

route:
  receiver: noop
  group_by:
    - alertname
    - severity
  group_wait: 30s
  group_interval: 5m
  repeat_interval: 4h

receivers:
  - name: noop
"@
}

function New-WebhookConfig {
    $url = Get-RequiredValue -Name "ALERTMANAGER_WEBHOOK_URL" -Value $WebhookUrl
    return @"
global:
  resolve_timeout: 5m

route:
  receiver: webhook
  group_by:
    - alertname
    - severity
  group_wait: 30s
  group_interval: 5m
  repeat_interval: 4h

receivers:
  - name: webhook
    webhook_configs:
      - url: $(ConvertTo-YamlScalar $url)
        send_resolved: true
"@
}

function New-BackendWebhookConfig {
    $url = if ([string]::IsNullOrWhiteSpace($BackendWebhookUrl)) {
        "http://backend:8080/api/internal/alertmanager/webhook"
    } else {
        $BackendWebhookUrl.Trim()
    }
    $token = Get-RequiredValue -Name "ALERTMANAGER_WEBHOOK_TOKEN" -Value $AlertmanagerWebhookToken

    return @"
global:
  resolve_timeout: 5m

route:
  receiver: backend-notification-channels
  group_by:
    - alertname
    - severity
  group_wait: 30s
  group_interval: 5m
  repeat_interval: 4h

receivers:
  - name: backend-notification-channels
    webhook_configs:
      - url: $(ConvertTo-YamlScalar $url)
        send_resolved: true
        http_config:
          authorization:
            type: Bearer
            credentials: $(ConvertTo-YamlScalar $token)
"@
}

function New-EmailConfig {
    $to = Get-RequiredValue -Name "ALERTMANAGER_EMAIL_TO" -Value $EmailTo
    $smtp = Get-SmtpBlock
    return @"
$smtp

route:
  receiver: email
  group_by:
    - alertname
    - severity
  group_wait: 30s
  group_interval: 5m
  repeat_interval: 4h

receivers:
  - name: email
    email_configs:
      - to: $(ConvertTo-YamlScalar $to)
        send_resolved: true
"@
}

function New-ProductionConfig {
    $criticalWebhook = Get-RequiredValue -Name "ALERTMANAGER_CRITICAL_WEBHOOK_URL" -Value $CriticalWebhookUrl
    $criticalEmail = Get-RequiredValue -Name "ALERTMANAGER_CRITICAL_EMAIL_TO" -Value $CriticalEmailTo
    $warningEmail = Get-RequiredValue -Name "ALERTMANAGER_WARNING_EMAIL_TO" -Value $WarningEmailTo
    $smtp = Get-SmtpBlock
    return @"
$smtp

route:
  receiver: warning-email
  group_by:
    - alertname
    - severity
  group_wait: 30s
  group_interval: 5m
  repeat_interval: 4h
  routes:
    - matchers:
        - severity="critical"
      receiver: critical-oncall
      repeat_interval: 30m
    - matchers:
        - severity="warning"
      receiver: warning-email

receivers:
  - name: critical-oncall
    webhook_configs:
      - url: $(ConvertTo-YamlScalar $criticalWebhook)
        send_resolved: true
    email_configs:
      - to: $(ConvertTo-YamlScalar $criticalEmail)
        send_resolved: true

  - name: warning-email
    email_configs:
      - to: $(ConvertTo-YamlScalar $warningEmail)
        send_resolved: true
"@
}

switch ($Mode) {
    "noop" { $config = New-NoopConfig }
    "backend" { $config = New-BackendWebhookConfig }
    "webhook" { $config = New-WebhookConfig }
    "email" { $config = New-EmailConfig }
    "production" { $config = New-ProductionConfig }
}

$outPath = Resolve-RepoPath $OutputPath
$directory = Split-Path -Parent $outPath
if (-not [string]::IsNullOrWhiteSpace($directory)) {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
}

Set-Content -LiteralPath $outPath -Value $config -Encoding UTF8
Write-Host "Rendered Alertmanager config: $outPath"

if ($Check) {
    $amtool = Get-Command "amtool" -ErrorAction SilentlyContinue
    if (-not $amtool) {
        throw "amtool is required when -Check is set."
    }

    & amtool check-config $outPath
    if ($LASTEXITCODE -ne 0) {
        throw "amtool check-config failed."
    }
}
