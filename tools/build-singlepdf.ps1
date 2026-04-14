param(
    [switch]$Debug,
    [string]$ExtensionOutDir = "",
    [string]$BinaryOutDir = ""
)

$ErrorActionPreference = "Stop"

function Get-FullPath {
    param(
        [string]$BasePath,
        [string]$Candidate
    )

    if ([System.IO.Path]::IsPathRooted($Candidate)) {
        return [System.IO.Path]::GetFullPath($Candidate)
    }

    return [System.IO.Path]::GetFullPath((Join-Path $BasePath $Candidate))
}

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$profile = if ($Debug) { "debug" } else { "release" }
$targetExe = Join-Path $workspaceRoot "target\$profile\singlepdf.exe"
$targetPdb = Join-Path $workspaceRoot "target\$profile\singlepdf.pdb"
$buildRoot = Join-Path $workspaceRoot "build"
$extensionOutput = if ($ExtensionOutDir) {
    Get-FullPath -BasePath $workspaceRoot -Candidate $ExtensionOutDir
} else {
    Join-Path $buildRoot "extension"
}
$binaryOutputDir = if ($BinaryOutDir) {
    Get-FullPath -BasePath $workspaceRoot -Candidate $BinaryOutDir
} else {
    Join-Path $buildRoot $profile
}
$binaryOutput = Join-Path $binaryOutputDir "singlepdf.exe"
$binaryOutputForCommand = $binaryOutput.Replace("'", "''")

Push-Location $workspaceRoot
try {
    $cargoArgs = @("build")
    if (-not $Debug) {
        $cargoArgs += "--release"
    }

    Write-Host "Building Rust binary ($profile)..."
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed."
    }

    if (-not (Test-Path -LiteralPath $targetExe)) {
        throw "Expected built binary at $targetExe"
    }

    New-Item -ItemType Directory -Force -Path $binaryOutputDir | Out-Null
    Copy-Item -LiteralPath $targetExe -Destination $binaryOutput -Force
    if (Test-Path -LiteralPath $targetPdb) {
        Copy-Item -LiteralPath $targetPdb -Destination (Join-Path $binaryOutputDir "singlepdf.pdb") -Force
    }

    Write-Host "Preparing loadable extension folder..."
    & node .\tools\build-extension.mjs --out-dir $extensionOutput
    if ($LASTEXITCODE -ne 0) {
        throw "Extension build failed."
    }

    Write-Host ""
    Write-Host "Build outputs"
    Write-Host "  Binary   : $binaryOutput"
    Write-Host "  Extension: $extensionOutput"
    Write-Host ""
    Write-Host "Firefox native host install"
    Write-Host "  .\tools\install-native-host.ps1 -ExePath '$binaryOutputForCommand' -FirefoxOnly"
    Write-Host ""
    Write-Host "Edge native host install"
    Write-Host "  .\tools\install-native-host.ps1 -ExePath '$binaryOutputForCommand' -EdgeOnly -EdgeExtensionId <your-edge-extension-id>"
}
finally {
    Pop-Location
}
