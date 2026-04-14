param(
    [string]$HostName = "singlepdf.host",
    [string]$ExePath = "",
    [string]$FirefoxExtensionId = "singlepdf@example.local",
    [string]$EdgeExtensionId = "",
    [switch]$FirefoxOnly,
    [switch]$EdgeOnly
)

$ErrorActionPreference = "Stop"

function Ensure-Directory {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        New-Item -ItemType Directory -Path $Path | Out-Null
    }
}

function Resolve-ExePath {
    param([string]$Candidate)

    if ($Candidate) {
        return (Resolve-Path -LiteralPath $Candidate).Path
    }

    $workspaceRoot = Split-Path -Parent $PSScriptRoot
    $buildReleaseExe = Join-Path $workspaceRoot "build\release\singlepdf.exe"
    $buildDebugExe = Join-Path $workspaceRoot "build\debug\singlepdf.exe"
    $debugExe = Join-Path $workspaceRoot "target\debug\singlepdf.exe"
    $releaseExe = Join-Path $workspaceRoot "target\release\singlepdf.exe"

    if (Test-Path -LiteralPath $buildReleaseExe) {
        return (Resolve-Path -LiteralPath $buildReleaseExe).Path
    }
    if (Test-Path -LiteralPath $buildDebugExe) {
        return (Resolve-Path -LiteralPath $buildDebugExe).Path
    }
    if (Test-Path -LiteralPath $releaseExe) {
        return (Resolve-Path -LiteralPath $releaseExe).Path
    }
    if (Test-Path -LiteralPath $debugExe) {
        return (Resolve-Path -LiteralPath $debugExe).Path
    }

    throw "Could not find singlepdf.exe. Pass -ExePath or build the project first."
}

function Write-JsonFile {
    param(
        [string]$Path,
        [hashtable]$Data
    )
    $json = $Data | ConvertTo-Json -Depth 10
    Set-Content -LiteralPath $Path -Value $json
}

function Install-FirefoxHost {
    param(
        [string]$HostName,
        [string]$ExePath,
        [string]$FirefoxExtensionId
    )

    $baseDir = Join-Path $env:LOCALAPPDATA "SinglePDF\NativeMessagingHosts"
    Ensure-Directory $baseDir
    $manifestPath = Join-Path $baseDir "$HostName.firefox.json"

    Write-JsonFile -Path $manifestPath -Data @{
        name = $HostName
        description = "SinglePDF native messaging host for Firefox"
        path = $ExePath
        type = "stdio"
        allowed_extensions = @($FirefoxExtensionId)
    }

    $regPath = "HKCU:\Software\Mozilla\NativeMessagingHosts\$HostName"
    New-Item -Path $regPath -Force | Out-Null
    New-ItemProperty -Path $regPath -Name "(default)" -Value $manifestPath -PropertyType String -Force | Out-Null

    [PSCustomObject]@{
        Browser = "Firefox"
        Manifest = $manifestPath
        Registry = $regPath
    }
}

function Install-EdgeHost {
    param(
        [string]$HostName,
        [string]$ExePath,
        [string]$EdgeExtensionId
    )

    if (-not $EdgeExtensionId) {
        throw "Edge installation requires -EdgeExtensionId."
    }

    $baseDir = Join-Path $env:LOCALAPPDATA "SinglePDF\NativeMessagingHosts"
    Ensure-Directory $baseDir
    $manifestPath = Join-Path $baseDir "$HostName.edge.json"

    Write-JsonFile -Path $manifestPath -Data @{
        name = $HostName
        description = "SinglePDF native messaging host for Edge"
        path = $ExePath
        type = "stdio"
        allowed_origins = @("chrome-extension://$EdgeExtensionId/")
    }

    $regPath = "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$HostName"
    New-Item -Path $regPath -Force | Out-Null
    New-ItemProperty -Path $regPath -Name "(default)" -Value $manifestPath -PropertyType String -Force | Out-Null

    [PSCustomObject]@{
        Browser = "Edge"
        Manifest = $manifestPath
        Registry = $regPath
    }
}

$resolvedExe = Resolve-ExePath -Candidate $ExePath
$installFirefox = -not $EdgeOnly
$installEdge = -not $FirefoxOnly

$results = @()
if ($installFirefox) {
    $results += Install-FirefoxHost -HostName $HostName -ExePath $resolvedExe -FirefoxExtensionId $FirefoxExtensionId
}
if ($installEdge) {
    $results += Install-EdgeHost -HostName $HostName -ExePath $resolvedExe -EdgeExtensionId $EdgeExtensionId
}

$results | Format-Table -AutoSize
