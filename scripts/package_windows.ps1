param(
    [string]$Version = "1.2",
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"

$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
$distRoot = Join-Path $repo "target\dist"
$packageName = "AA-Converter-Windows-x64-v$Version"
$stage = Join-Path $distRoot $packageName
$zipPath = Join-Path $distRoot "$packageName.zip"

Push-Location $repo
try {
    cargo build -p aa-egui --release

    $resolvedDist = [System.IO.Path]::GetFullPath($distRoot)
    $resolvedStage = [System.IO.Path]::GetFullPath($stage)
    if (-not $resolvedStage.StartsWith($resolvedDist, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to package outside target\dist: $resolvedStage"
    }

    if (Test-Path $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stage | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $stage "assets\fonts") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $stage "assets\icons") -Force | Out-Null

    Copy-Item -LiteralPath (Join-Path $repo "target\release\aa-egui.exe") -Destination (Join-Path $stage "AA Converter.exe")
    Copy-Item -LiteralPath (Join-Path $repo "assets\model_catalog.json") -Destination (Join-Path $stage "model_catalog.json")
    Copy-Item -LiteralPath (Join-Path $repo "THIRD_PARTY_NOTICES.md") -Destination (Join-Path $stage "THIRD_PARTY_NOTICES.md")
    Copy-Item -LiteralPath (Join-Path $repo "README.md") -Destination (Join-Path $stage "README.md")
    Copy-Item -LiteralPath (Join-Path $repo "assets\fonts\Saitamaar-Regular.ttf") -Destination (Join-Path $stage "assets\fonts\Saitamaar-Regular.ttf")
    Copy-Item -LiteralPath (Join-Path $repo "assets\fonts\Saitamaar-OFL.txt") -Destination (Join-Path $stage "assets\fonts\Saitamaar-OFL.txt")
    Copy-Item -LiteralPath (Join-Path $repo "assets\icons\aa-converter-icon.ico") -Destination (Join-Path $stage "assets\icons\aa-converter-icon.ico")
    Copy-Item -LiteralPath (Join-Path $repo "assets\icons\aa-converter-icon.png") -Destination (Join-Path $stage "assets\icons\aa-converter-icon.png")

    if (Test-Path $zipPath) {
        Remove-Item -LiteralPath $zipPath -Force
    }
    Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zipPath
    Write-Host "Package created: $zipPath"
}
finally {
    Pop-Location
}
