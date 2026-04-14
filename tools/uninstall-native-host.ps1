param(
    [string]$HostName = "singlepdf.host",
    [switch]$FirefoxOnly,
    [switch]$EdgeOnly
)

$ErrorActionPreference = "Stop"

function Remove-RegistryKeyIfExists {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
}

$baseDir = Join-Path $env:LOCALAPPDATA "SinglePDF\NativeMessagingHosts"
$removeFirefox = -not $EdgeOnly
$removeEdge = -not $FirefoxOnly

if ($removeFirefox) {
    Remove-RegistryKeyIfExists -Path "HKCU:\Software\Mozilla\NativeMessagingHosts\$HostName"
    $manifestPath = Join-Path $baseDir "$HostName.firefox.json"
    if (Test-Path -LiteralPath $manifestPath) {
        Remove-Item -LiteralPath $manifestPath -Force
    }
}

if ($removeEdge) {
    Remove-RegistryKeyIfExists -Path "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$HostName"
    $manifestPath = Join-Path $baseDir "$HostName.edge.json"
    if (Test-Path -LiteralPath $manifestPath) {
        Remove-Item -LiteralPath $manifestPath -Force
    }
}
