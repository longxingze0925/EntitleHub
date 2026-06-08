#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$HostName = "localhost",
    [string]$BaseUrl = $env:AI_GATEWAY_BASE_URL,
    [string]$ApiKey = $env:AI_GATEWAY_API_KEY,
    [string]$ChatModel = $env:AI_GATEWAY_CHAT_MODEL,
    [string]$EmbeddingModel = $env:AI_GATEWAY_EMBEDDING_MODEL,
    [string]$ImageModel = $env:AI_GATEWAY_IMAGE_MODEL,
    [int]$TimeoutSeconds = 60,
    [switch]$SkipAssetDownload
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
    if (-not (Test-Path -LiteralPath $Path)) {
        return $values
    }

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

function Join-Url {
    param(
        [string]$Root,
        [string]$Path
    )

    return "$($Root.TrimEnd('/'))/$($Path.TrimStart('/'))"
}

function Invoke-AiJson {
    param(
        [string]$Name,
        [string]$Url,
        [hashtable]$Headers,
        [object]$Body
    )

    Write-Host "==> $Name"
    $json = $Body | ConvertTo-Json -Depth 20 -Compress
    try {
        return Invoke-RestMethod `
            -Method Post `
            -Uri $Url `
            -Headers $Headers `
            -ContentType "application/json" `
            -Body $json `
            -TimeoutSec $TimeoutSeconds
    } catch {
        $message = $_.Exception.Message
        if ($_.ErrorDetails -and $_.ErrorDetails.Message) {
            $message = $_.ErrorDetails.Message
        }
        throw "$Name failed. $message"
    }
}

function Resolve-AssetUrl {
    param(
        [string]$GatewayRoot,
        [string]$AssetUrl
    )

    if ($AssetUrl -match '^https?://') {
        return $AssetUrl
    }

    $root = $GatewayRoot.TrimEnd('/')
    if ($root.EndsWith('/v1')) {
        $root = $root.Substring(0, $root.Length - 3)
    }

    return Join-Url -Root $root -Path $AssetUrl
}

$envPath = Resolve-RepoPath $EnvFile
$envValues = Read-EnvFile -Path $envPath

if ([string]::IsNullOrWhiteSpace($BaseUrl)) {
    $backendPort = Get-PortValue -Values $envValues -Key "BACKEND_PORT" -Default 8080
    $BaseUrl = "http://${HostName}:$backendPort/v1"
}

if ([string]::IsNullOrWhiteSpace($ApiKey)) {
    throw "AI gateway API key is required. Set AI_GATEWAY_API_KEY or pass -ApiKey."
}

$headers = @{
    Authorization = "Bearer $ApiKey"
}

Write-Host "==> List models"
$models = Invoke-RestMethod `
    -Method Get `
    -Uri (Join-Url -Root $BaseUrl -Path "models") `
    -Headers $headers `
    -TimeoutSec $TimeoutSeconds
if ($null -eq $models.data) {
    throw "Model list response did not include data."
}
Write-Host "Models returned: $(@($models.data).Count)"

if (-not [string]::IsNullOrWhiteSpace($ChatModel)) {
    $chat = Invoke-AiJson `
        -Name "Chat completions" `
        -Url (Join-Url -Root $BaseUrl -Path "chat/completions") `
        -Headers $headers `
        -Body @{
            model = $ChatModel
            messages = @(@{ role = "user"; content = "hello" })
            max_tokens = 16
        }
    if ($null -eq $chat.choices -or @($chat.choices).Count -lt 1) {
        throw "Chat response did not include choices."
    }
}

if (-not [string]::IsNullOrWhiteSpace($EmbeddingModel)) {
    $embedding = Invoke-AiJson `
        -Name "Embeddings" `
        -Url (Join-Url -Root $BaseUrl -Path "embeddings") `
        -Headers $headers `
        -Body @{
            model = $EmbeddingModel
            input = "hello"
        }
    if ($null -eq $embedding.data -or @($embedding.data).Count -lt 1) {
        throw "Embedding response did not include data."
    }
}

if (-not [string]::IsNullOrWhiteSpace($ImageModel)) {
    $image = Invoke-AiJson `
        -Name "Image generations" `
        -Url (Join-Url -Root $BaseUrl -Path "images/generations") `
        -Headers $headers `
        -Body @{
            model = $ImageModel
            prompt = "A simple product poster with clean typography"
            n = 1
            size = "1024x1024"
        }
    if ($null -eq $image.data -or @($image.data).Count -lt 1 -or [string]::IsNullOrWhiteSpace($image.data[0].url)) {
        throw "Image response did not include a cached url."
    }

    if (-not $SkipAssetDownload) {
        $assetUrl = Resolve-AssetUrl -GatewayRoot $BaseUrl -AssetUrl $image.data[0].url
        Write-Host "==> Cached asset download"
        $assetResponse = Invoke-WebRequest -Method Get -Uri $assetUrl -TimeoutSec $TimeoutSeconds
        if ($assetResponse.StatusCode -ne 200 -or $assetResponse.RawContentLength -lt 1) {
            throw "Cached asset download did not return bytes."
        }
    }
}

Write-Host "AI gateway smoke passed."
