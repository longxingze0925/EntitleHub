#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$HostName = "localhost",
    [int]$TimeoutSeconds = 10,
    [string]$OwnerEmail = $env:INIT_OWNER_EMAIL,
    [string]$OwnerPassword = $env:INIT_OWNER_PASSWORD,
    [string]$CredentialFile = ".tools/init-owner-smoke.env",
    [string]$SmokeAppSlug = "expiry-smoke",
    [int]$ExpirySeconds = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Net.Http

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$devicePublicKey = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"

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

function New-StrongSmokePassword {
    $bytes = New-Object byte[] 24
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }

    $token = [Convert]::ToBase64String($bytes).TrimEnd("=").Replace("+", "-").Replace("/", "_")
    return "Aa1!$token"
}

function Get-CsrfToken {
    param(
        [Microsoft.PowerShell.Commands.WebRequestSession]$WebSession,
        [string]$BackendUrl
    )

    $cookies = $WebSession.Cookies.GetCookies([Uri]$BackendUrl)
    foreach ($cookie in $cookies) {
        if ($cookie.Name -eq "admin_csrf" -and -not [string]::IsNullOrWhiteSpace($cookie.Value)) {
            return $cookie.Value
        }
    }

    throw "admin_csrf cookie was not returned by admin login."
}

function Invoke-RawJson {
    param(
        [ValidateSet("GET", "POST", "PUT")]
        [string]$Method,
        [string]$Uri,
        [object]$Body = $null,
        [Microsoft.PowerShell.Commands.WebRequestSession]$WebSession = $null,
        [hashtable]$Headers = $null
    )

    $params = @{
        Uri = $Uri
        Method = $Method
        TimeoutSec = $TimeoutSeconds
    }
    if ($WebSession) {
        $params.WebSession = $WebSession
    }
    if ($Headers) {
        $params.Headers = $Headers
    }
    if ($null -ne $Body) {
        $params.Body = ($Body | ConvertTo-Json -Depth 12 -Compress)
        $params.ContentType = "application/json"
    }
    if ((Get-Command Invoke-WebRequest).Parameters.ContainsKey("UseBasicParsing")) {
        $params.UseBasicParsing = $true
    }
    if ((Get-Command Invoke-WebRequest).Parameters.ContainsKey("SkipHttpErrorCheck")) {
        $params.SkipHttpErrorCheck = $true
        $response = Invoke-WebRequest @params
        return [pscustomobject]@{ Status = [int]$response.StatusCode; Body = [string]$response.Content }
    }

    try {
        $response = Invoke-WebRequest @params
        return [pscustomobject]@{ Status = [int]$response.StatusCode; Body = [string]$response.Content }
    } catch {
        $status = 0
        $content = $_.Exception.Message
        $responseProperty = $_.Exception.PSObject.Properties["Response"]
        $response = if ($responseProperty) { $responseProperty.Value } else { $null }
        if ($response) {
            $status = [int]$response.StatusCode
            $stream = $response.GetResponseStream()
            if ($stream) {
                $reader = New-Object System.IO.StreamReader($stream)
                try {
                    $content = $reader.ReadToEnd()
                } finally {
                    $reader.Dispose()
                }
            }
        }

        return [pscustomobject]@{ Status = $status; Body = $content }
    }
}

function Assert-ApiEnvelope {
    param(
        [object]$Envelope,
        [string]$Name
    )

    if (-not (Test-ObjectProperty -Value $Envelope -Name "code")) {
        throw "$Name failed: response envelope is missing code: $($Envelope | ConvertTo-Json -Depth 12 -Compress)"
    }

    if ($Envelope.code -ne 0) {
        throw "$Name failed: code=$($Envelope.code) message=$($Envelope.message)"
    }

    if (-not $Envelope.data) {
        throw "$Name failed: missing data."
    }

    return $Envelope.data
}

function Test-ObjectProperty {
    param(
        [object]$Value,
        [string]$Name
    )

    return $null -ne (, $Value | Get-Member -Name $Name -MemberType NoteProperty,Property -ErrorAction SilentlyContinue)
}

function Invoke-AdminJson {
    param(
        [ValidateSet("GET", "POST", "PUT")]
        [string]$Method,
        [string]$Path,
        [object]$Body = $null
    )

    $headers = @{ "X-CSRF-Token" = $csrfToken }
    $response = Invoke-RawJson -Method $Method -Uri "$backendUrl$Path" -Body $Body -WebSession $webSession -Headers $headers
    try {
        $envelope = $response.Body | ConvertFrom-Json
    } catch {
        throw "$Path returned HTTP $($response.Status) with non-JSON body: $($response.Body)"
    }

    return Assert-ApiEnvelope -Envelope $envelope -Name $Path
}

function Invoke-ClientJson {
    param(
        [string]$Path,
        [object]$Body
    )

    $json = $Body | ConvertTo-Json -Depth 12 -Compress
    $client = New-Object System.Net.Http.HttpClient
    try {
        $content = New-Object System.Net.Http.StringContent($json, [System.Text.Encoding]::UTF8, "application/json")
        $response = $client.PostAsync("$backendUrl$Path", $content).Result
        $bodyText = $response.Content.ReadAsStringAsync().Result
        return [pscustomobject]@{
            Status = [int]$response.StatusCode
            Body = $bodyText
        }
    } finally {
        $client.Dispose()
    }
}

function Assert-ClientOk {
    param(
        [object]$Result,
        [string]$Name
    )

    if ($Result.Status -ne 200) {
        throw "$Name expected HTTP 200, got HTTP $($Result.Status): $($Result.Body)"
    }

    $envelope = $Result.Body | ConvertFrom-Json
    return Assert-ApiEnvelope -Envelope $envelope -Name $Name
}

function Assert-ClientError {
    param(
        [object]$Result,
        [int]$Status,
        [int]$Code,
        [string]$Message,
        [string]$Name
    )

    $envelope = $Result.Body | ConvertFrom-Json
    if (-not (Test-ObjectProperty -Value $envelope -Name "code")) {
        throw "$Name expected HTTP $Status code=$Code message=$Message, got malformed body: $($Result.Body)"
    }

    if ($Result.Status -ne $Status -or $envelope.code -ne $Code -or $envelope.message -ne $Message) {
        throw "$Name expected HTTP $Status code=$Code message=$Message, got HTTP $($Result.Status) code=$($envelope.code) message=$($envelope.message)"
    }

    Write-Host "$Name => HTTP $($Result.Status), code=$($envelope.code), message=$($envelope.message)"
}

$envPath = Resolve-RepoPath $EnvFile
if (-not (Test-Path -LiteralPath $envPath)) {
    throw "Env file not found: $envPath"
}

$credentialPath = Resolve-RepoPath $CredentialFile
if ((Test-Path -LiteralPath $credentialPath) -and ([string]::IsNullOrWhiteSpace($OwnerEmail) -or [string]::IsNullOrWhiteSpace($OwnerPassword))) {
    $credentialValues = Read-EnvFile -Path $credentialPath
    $OwnerEmail = Get-EnvValue -Values $credentialValues -Key "INIT_OWNER_EMAIL" -Default $OwnerEmail
    $OwnerPassword = Get-EnvValue -Values $credentialValues -Key "INIT_OWNER_PASSWORD" -Default $OwnerPassword
}

if ([string]::IsNullOrWhiteSpace($OwnerEmail) -or [string]::IsNullOrWhiteSpace($OwnerPassword)) {
    throw "Owner credentials are required. Run ops/smoke-init-owner.ps1 -RunMigrations first, or set INIT_OWNER_EMAIL and INIT_OWNER_PASSWORD."
}

$envValues = Read-EnvFile -Path $envPath
$backendPort = Get-PortValue -Values $envValues -Key "BACKEND_HOST_PORT" -Default 8080
$backendUrl = "http://${HostName}:$backendPort"

Write-Host "==> Admin login"
$loginPayload = @{
    email = $OwnerEmail
    password = $OwnerPassword
} | ConvertTo-Json -Compress

$webSession = $null
$loginResponse = Invoke-RestMethod -Uri "$backendUrl/api/auth/login" -Method Post -Body $loginPayload -ContentType "application/json" -TimeoutSec $TimeoutSeconds -SessionVariable webSession
Assert-ApiEnvelope -Envelope $loginResponse -Name "admin login" | Out-Null
$csrfToken = Get-CsrfToken -WebSession $webSession -BackendUrl $backendUrl

Write-Host "==> Ensure expiry smoke application"
$appList = Invoke-AdminJson -Method GET -Path "/api/admin/apps?keyword=$([Uri]::EscapeDataString($SmokeAppSlug))"
$app = $appList.items | Where-Object { $_.slug -eq $SmokeAppSlug } | Select-Object -First 1
if (-not $app) {
    $appData = Invoke-AdminJson -Method POST -Path "/api/admin/apps" -Body @{
        name = "Expiry Smoke App"
        slug = $SmokeAppSlug
        auth_mode = "both"
        heartbeat_interval_seconds = 2
        offline_tolerance_seconds = 4
        max_devices_default = 2
        metadata = @{
            smoke = "expiry-flow"
        }
    }
    $app = $appData.application
} elseif ($app.auth_mode -ne "both") {
    $appData = Invoke-AdminJson -Method PUT -Path "/api/admin/apps/$($app.id)" -Body @{
        auth_mode = "both"
        status = "active"
    }
    $app = $appData.application
}

if (-not $app.id -or -not $app.app_key) {
    throw "Expiry smoke application response did not include id and app_key."
}

$suffix = [Guid]::NewGuid().ToString("N")
$customerEmail = "expiry-smoke-$suffix@example.test"
$customerPassword = New-StrongSmokePassword

Write-Host "==> Create expiry smoke customer"
$customerData = Invoke-AdminJson -Method POST -Path "/api/admin/customers" -Body @{
    email = $customerEmail
    name = "Expiry Smoke Customer"
    password = $customerPassword
    metadata = @{
        smoke = "expiry-flow"
    }
}
$customer = $customerData.customer
if (-not $customer.id) {
    throw "Expiry smoke customer response did not include id."
}

Write-Host "==> Verify expired license blocks refresh"
$licenseExpiresAt = (Get-Date).ToUniversalTime().AddSeconds($ExpirySeconds).ToString("yyyy-MM-ddTHH:mm:ssZ")
$licenseData = Invoke-AdminJson -Method POST -Path "/api/admin/licenses" -Body @{
    app_id = $app.id
    customer_id = $customer.id
    max_devices = 1
    starts_at = (Get-Date).ToUniversalTime().AddMinutes(-1).ToString("yyyy-MM-ddTHH:mm:ssZ")
    expires_at = $licenseExpiresAt
    features = @("expiry-smoke")
    metadata = @{
        smoke = "expiry-flow"
        kind = "license"
    }
}

$licenseSession = Assert-ClientOk -Result (Invoke-ClientJson -Path "/api/client/auth/activate" -Body @{
    app_key = $app.app_key
    license_key = $licenseData.license_key
    machine_id = "expiry-license-$suffix"
    device_name = "Expiry License Smoke"
    os = "smoke"
    app_version = "expiry-smoke"
    device_public_key = $devicePublicKey
}) -Name "license activation before expiry"

Start-Sleep -Seconds ($ExpirySeconds + 2)
Assert-ClientError -Result (Invoke-ClientJson -Path "/api/client/auth/refresh" -Body @{
    refresh_token = $licenseSession.refresh_token
}) -Status 403 -Code 40305 -Message "license_expired" -Name "expired license refresh"

Write-Host "==> Verify expired subscription blocks refresh"
$subscriptionExpiresAt = (Get-Date).ToUniversalTime().AddSeconds($ExpirySeconds).ToString("yyyy-MM-ddTHH:mm:ssZ")
Invoke-AdminJson -Method POST -Path "/api/admin/subscriptions" -Body @{
    app_id = $app.id
    customer_id = $customer.id
    plan = "expiry-smoke"
    max_devices = 1
    starts_at = (Get-Date).ToUniversalTime().AddMinutes(-1).ToString("yyyy-MM-ddTHH:mm:ssZ")
    expires_at = $subscriptionExpiresAt
    features = @("expiry-smoke")
    metadata = @{
        smoke = "expiry-flow"
        kind = "subscription"
    }
} | Out-Null

$subscriptionSession = Assert-ClientOk -Result (Invoke-ClientJson -Path "/api/client/auth/login" -Body @{
    app_key = $app.app_key
    email = $customerEmail
    password = $customerPassword
    machine_id = "expiry-subscription-$suffix"
    device_name = "Expiry Subscription Smoke"
    os = "smoke"
    app_version = "expiry-smoke"
    device_public_key = $devicePublicKey
}) -Name "subscription login before expiry"

Start-Sleep -Seconds ($ExpirySeconds + 2)
Assert-ClientError -Result (Invoke-ClientJson -Path "/api/client/auth/refresh" -Body @{
    refresh_token = $subscriptionSession.refresh_token
}) -Status 403 -Code 40306 -Message "subscription_inactive" -Name "expired subscription refresh"

Write-Host "Expiry flow smoke passed."
