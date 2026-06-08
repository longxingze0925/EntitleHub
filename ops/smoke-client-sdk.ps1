#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$HostName = "localhost",
    [int]$TimeoutSeconds = 10,
    [string]$OwnerEmail = $env:INIT_OWNER_EMAIL,
    [string]$OwnerPassword = $env:INIT_OWNER_PASSWORD,
    [string]$CredentialFile = ".tools/init-owner-smoke.env",
    [string]$SmokeAppSlug = "sdk-smoke"
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

function Assert-ApiEnvelope {
    param(
        [object]$Envelope,
        [string]$Name
    )

    if ($Envelope.code -ne 0) {
        throw "$Name failed: code=$($Envelope.code) message=$($Envelope.message)"
    }

    if (-not $Envelope.data) {
        throw "$Name failed: missing data."
    }
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

function Invoke-AdminJson {
    param(
        [ValidateSet("GET", "POST", "PUT")]
        [string]$Method,
        [string]$Path,
        [object]$Body = $null
    )

    $headers = @{ "X-CSRF-Token" = $csrfToken }
    $uri = "$backendUrl$Path"
    if ($Method -eq "GET") {
        $response = Invoke-RestMethod -Uri $uri -Method Get -TimeoutSec $TimeoutSeconds -WebSession $webSession
    } else {
        $json = if ($null -eq $Body) { "{}" } else { $Body | ConvertTo-Json -Depth 12 -Compress }
        $response = Invoke-RestMethod -Uri $uri -Method $Method -Body $json -ContentType "application/json" -Headers $headers -TimeoutSec $TimeoutSeconds -WebSession $webSession
    }

    Assert-ApiEnvelope -Envelope $response -Name $Path
    return $response.data
}

function Set-ProcessEnv {
    param([hashtable]$Values)

    $previous = @{}
    foreach ($key in $Values.Keys) {
        $previous[$key] = [Environment]::GetEnvironmentVariable($key, "Process")
        [Environment]::SetEnvironmentVariable($key, [string]$Values[$key], "Process")
    }

    return $previous
}

function Restore-ProcessEnv {
    param([hashtable]$Values)

    foreach ($key in $Values.Keys) {
        [Environment]::SetEnvironmentVariable($key, $Values[$key], "Process")
    }
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
$jwtIssuer = Get-EnvValue -Values $envValues -Key "JWT_ISSUER" -Default $backendUrl
$jwtAudience = Get-EnvValue -Values $envValues -Key "JWT_AUDIENCE" -Default "client-sdk"

Write-Host "==> Admin login"
$loginPayload = @{
    email = $OwnerEmail
    password = $OwnerPassword
} | ConvertTo-Json -Compress

$webSession = $null
$loginResponse = Invoke-RestMethod -Uri "$backendUrl/api/auth/login" -Method Post -Body $loginPayload -ContentType "application/json" -TimeoutSec $TimeoutSeconds -SessionVariable webSession
Assert-ApiEnvelope -Envelope $loginResponse -Name "admin login"
$csrfToken = Get-CsrfToken -WebSession $webSession -BackendUrl $backendUrl

Write-Host "==> Ensure SDK smoke application"
$appList = Invoke-AdminJson -Method GET -Path "/api/admin/apps?keyword=$([Uri]::EscapeDataString($SmokeAppSlug))"
$app = $appList.items | Where-Object { $_.slug -eq $SmokeAppSlug } | Select-Object -First 1
if (-not $app) {
    $appData = Invoke-AdminJson -Method POST -Path "/api/admin/apps" -Body @{
        name = "SDK Smoke App"
        slug = $SmokeAppSlug
        auth_mode = "both"
        heartbeat_interval_seconds = 60
        offline_tolerance_seconds = 120
        max_devices_default = 2
        metadata = @{
            smoke = "client-sdk"
        }
    }
    $app = $appData.application
    if (-not $app) {
        $app = [pscustomobject]@{
            id = $appData.id
            app_key = $appData.app_key
            slug = $SmokeAppSlug
        }
    }
} elseif ($app.auth_mode -ne "both") {
    $appData = Invoke-AdminJson -Method PUT -Path "/api/admin/apps/$($app.id)" -Body @{
        auth_mode = "both"
        status = "active"
    }
    $app = $appData.application
}

if (-not $app.id -or -not $app.app_key) {
    throw "SDK smoke application response did not include id and app_key."
}

Write-Host "==> Create short-lived SDK smoke license"
$expiresAt = (Get-Date).ToUniversalTime().AddDays(7).ToString("yyyy-MM-ddTHH:mm:ssZ")
$licenseData = Invoke-AdminJson -Method POST -Path "/api/admin/licenses" -Body @{
    app_id = $app.id
    max_devices = 1
    expires_at = $expiresAt
    features = @("sdk-smoke")
    metadata = @{
        smoke = "client-sdk"
        app_slug = $SmokeAppSlug
    }
}

if (-not $licenseData.license_key) {
    throw "License creation response did not include license_key."
}

Write-Host "==> Create SDK AI smoke customers"
$suffix = [Guid]::NewGuid().ToString("N")
$noSubscriptionEmail = "sdk-ai-nosub-$suffix@example.test"
$noSubscriptionPassword = New-StrongSmokePassword
$subscriptionEmail = "sdk-ai-sub-$suffix@example.test"
$subscriptionPassword = New-StrongSmokePassword

$noSubscriptionCustomerData = Invoke-AdminJson -Method POST -Path "/api/admin/customers" -Body @{
    email = $noSubscriptionEmail
    name = "SDK AI Smoke No Subscription"
    password = $noSubscriptionPassword
    metadata = @{
        smoke = "client-sdk-ai"
        kind = "no-subscription"
    }
}
$subscriptionCustomerData = Invoke-AdminJson -Method POST -Path "/api/admin/customers" -Body @{
    email = $subscriptionEmail
    name = "SDK AI Smoke Subscription"
    password = $subscriptionPassword
    metadata = @{
        smoke = "client-sdk-ai"
        kind = "subscription"
    }
}

if (-not $noSubscriptionCustomerData.customer.id -or -not $subscriptionCustomerData.customer.id) {
    throw "SDK AI smoke customer creation response did not include customer ids."
}

Write-Host "==> Create SDK AI smoke subscription"
$subscriptionExpiresAt = (Get-Date).ToUniversalTime().AddDays(7).ToString("yyyy-MM-ddTHH:mm:ssZ")
Invoke-AdminJson -Method POST -Path "/api/admin/subscriptions" -Body @{
    app_id = $app.id
    customer_id = $subscriptionCustomerData.customer.id
    plan = "sdk-ai-smoke"
    max_devices = 2
    expires_at = $subscriptionExpiresAt
    features = @("ai")
    metadata = @{
        smoke = "client-sdk-ai"
    }
} | Out-Null

Write-Host "==> Run Client SDK activation live backend test"
$envSnapshot = Set-ProcessEnv -Values @{
    SDK_SMOKE_BACKEND_URL = $backendUrl
    SDK_SMOKE_APP_KEY = $app.app_key
    SDK_SMOKE_LICENSE_KEY = $licenseData.license_key
    SDK_SMOKE_MACHINE_ID = "sdk-smoke-$([Guid]::NewGuid().ToString('N'))"
    SDK_SMOKE_JWT_ISSUER = $jwtIssuer
    SDK_SMOKE_JWT_AUDIENCE = $jwtAudience
}

try {
    & cargo test --manifest-path client-sdk/Cargo.toml --test live_backend live_backend_activation_refresh_and_heartbeat -- --ignored --exact --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "Client SDK activation live backend test failed with exit code $LASTEXITCODE."
    }
} finally {
    Restore-ProcessEnv -Values $envSnapshot
}

Write-Host "==> Run Client SDK AI subscription gate test without subscription"
$envSnapshot = Set-ProcessEnv -Values @{
    SDK_SMOKE_BACKEND_URL = $backendUrl
    SDK_SMOKE_APP_KEY = $app.app_key
    SDK_SMOKE_CUSTOMER_EMAIL = $noSubscriptionEmail
    SDK_SMOKE_CUSTOMER_PASSWORD = $noSubscriptionPassword
    SDK_SMOKE_AI_EXPECT_SUBSCRIPTION = "false"
    SDK_SMOKE_MACHINE_ID = "sdk-ai-nosub-$([Guid]::NewGuid().ToString('N'))"
}

try {
    & cargo test --manifest-path client-sdk/Cargo.toml --test live_backend live_backend_customer_login_ai_subscription_gate -- --ignored --exact --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "Client SDK AI no-subscription live backend test failed with exit code $LASTEXITCODE."
    }
} finally {
    Restore-ProcessEnv -Values $envSnapshot
}

Write-Host "==> Run Client SDK AI subscription gate test with active subscription"
$envSnapshot = Set-ProcessEnv -Values @{
    SDK_SMOKE_BACKEND_URL = $backendUrl
    SDK_SMOKE_APP_KEY = $app.app_key
    SDK_SMOKE_CUSTOMER_EMAIL = $subscriptionEmail
    SDK_SMOKE_CUSTOMER_PASSWORD = $subscriptionPassword
    SDK_SMOKE_AI_EXPECT_SUBSCRIPTION = "true"
    SDK_SMOKE_MACHINE_ID = "sdk-ai-sub-$([Guid]::NewGuid().ToString('N'))"
}

try {
    & cargo test --manifest-path client-sdk/Cargo.toml --test live_backend live_backend_customer_login_ai_subscription_gate -- --ignored --exact --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "Client SDK AI subscription live backend test failed with exit code $LASTEXITCODE."
    }
} finally {
    Restore-ProcessEnv -Values $envSnapshot
}

Write-Host "Client SDK live backend smoke passed."
