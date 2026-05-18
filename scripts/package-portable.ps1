param(
  [ValidateSet("debug", "release")]
  [string]$Profile = "release"
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetProfile = if ($Profile -eq "release") { "release" } else { "debug" }
$exe = Join-Path $root "src-tauri\target\$targetProfile\kairos_patch.exe"

if (-not (Test-Path $exe)) {
  throw "Executable not found: $exe. Run npm run tauri -- build --$Profile first."
}

$packageDir = Join-Path $root "dist-portable\KairosPatch"
$zipPath = Join-Path $root "dist-portable\KairosPatch-$Profile-portable.zip"
if (Test-Path $packageDir) {
  Remove-Item -LiteralPath $packageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $packageDir | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $packageDir "kairos_patch.exe") -Force

@"
Kairos Patch Portable

Run kairos_patch.exe directly. No installer is required.

Self-update source:
Publish release-index.json and KairosPatch-portable.zip as GitHub Release assets.
"@ | Set-Content -LiteralPath (Join-Path $packageDir "README.txt") -Encoding UTF8

if (Test-Path $zipPath) {
  Remove-Item -LiteralPath $zipPath -Force
}
Compress-Archive -Path (Join-Path $packageDir "*") -DestinationPath $zipPath -Force
$stream = [System.IO.File]::OpenRead($zipPath)
try {
  $sha = [System.Security.Cryptography.SHA256]::Create()
  $hashBytes = $sha.ComputeHash($stream)
  $hash = -join ($hashBytes | ForEach-Object { $_.ToString("x2") })
}
finally {
  if ($sha) { $sha.Dispose() }
  $stream.Dispose()
}
@{
  file = $zipPath
  sha256 = $hash
} | ConvertTo-Json | Set-Content -LiteralPath "$zipPath.sha256.json" -Encoding UTF8

Write-Host "Portable package: $zipPath"
Write-Host "SHA256: $hash"
