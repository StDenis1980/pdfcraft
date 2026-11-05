<#
.SYNOPSIS
  Build a Tinta Office preview release for Windows x64 on Denys's PC.

.DESCRIPTION
  For each app repository under -DevDir (C:\dev by default): update main, run that repo's
  packaging\windows\package.ps1, and copy the MSI and portable zip into
  <DevDir>\tinta-release\<Tag>\. Then write, in that folder:

    SHA256SUMS.txt          one "<sha256>  <file>" line per file (the site lists these)
    sbom\<repo>.cdx.json    CycloneDX SBOM of each app's Rust dependencies (needs cargo-cyclonedx:
                            cargo install cargo-cyclonedx; skipped with a warning without it)

  and prints the `gh release create` commands that publish the files as a pre-release in each
  fork (the apps' "Check for updates" reads the fork's GitHub Releases). Nothing is published by
  this script.

  Needs what each package.ps1 needs (Rust MSVC, WiX v5) and DOTNET_ROOT for wix.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File C:\dev\pdfcraft\packaging\windows\tinta-build-release.ps1 -Tag preview3
  powershell -ExecutionPolicy Bypass -File C:\dev\pdfcraft\packaging\windows\tinta-build-release.ps1 -Tag preview3-rc2 -Apps pdfcraft,wordcraft -SkipPull
#>
param(
  [Parameter(Mandatory = $true)] [string] $Tag,
  [string] $DevDir = 'C:\dev',
  [string[]] $Apps = @('pdfcraft', 'wordcraft', 'gridcraft', 'deckcraft', 'ibercorreo'),
  # craft-fonts checkout (Japanese UI fallback + OFL notices); pdfcraft expects it for every release.
  [string] $FontsDir = 'C:\dev\craft-fonts',
  [switch] $SkipPull
)
$ErrorActionPreference = 'Stop'
if (-not $env:DOTNET_ROOT -and (Test-Path 'C:\dev\tools\dotnet')) { $env:DOTNET_ROOT = 'C:\dev\tools\dotnet' }
if (Test-Path 'C:\dev\tools\wix') { $env:PATH = "C:\dev\tools\wix;$env:PATH" }
if ($FontsDir -and (Test-Path $FontsDir)) { $env:CRAFT_FONTS_DIR = $FontsDir } else { Write-Warning "no craft-fonts at '$FontsDir': builds without CRAFT_FONTS_DIR" }
# Tinta Correo's OAuth client IDs come from the user environment (setx); warn when missing.
foreach ($v in 'IBERCORREO_GOOGLE_CLIENT_ID', 'IBERCORREO_GOOGLE_CLIENT_SECRET', 'IBERCORREO_MICROSOFT_CLIENT_ID') {
  if (-not [Environment]::GetEnvironmentVariable($v) -and ($Apps -contains 'ibercorreo')) { Write-Warning "$v is not set: Correo will lack that sign-in" }
}
$Logs = Join-Path $DevDir "tinta-release\logs\$Tag"
New-Item -ItemType Directory -Force -Path $Logs | Out-Null

$Out = Join-Path $DevDir "tinta-release\$Tag"
New-Item -ItemType Directory -Force -Path (Join-Path $Out 'sbom') | Out-Null
$HasCycloneDx = [bool](cargo --list 2>$null | Select-String -SimpleMatch 'cyclonedx')
if (-not $HasCycloneDx) { Write-Warning 'cargo-cyclonedx is not installed: no SBOMs (cargo install cargo-cyclonedx)' }

foreach ($app in $Apps) {
  $repo = Join-Path $DevDir $app
  if (-not (Test-Path $repo)) { throw "$repo not found (git clone https://github.com/StDenis1980/$app $repo)" }
  Write-Output "==== $app"
  Push-Location $repo
  try {
    if (-not $SkipPull) {
      git fetch origin main; if ($LASTEXITCODE -ne 0) { throw "${app}: git fetch failed" }
      git checkout main; if ($LASTEXITCODE -ne 0) { throw "${app}: git checkout main failed" }
      git merge --ff-only origin/main; if ($LASTEXITCODE -ne 0) { throw "${app}: main has local commits" }
    }
    # WordCraft (since 2026-10-26) keeps its translations in craft-i18n: without
    # CRAFT_I18N_DIR the build is English only. Check out the pinned commit and require it.
    $env:CRAFT_I18N_DIR = $null; $env:CRAFT_I18N_REQUIRED = $null
    $revFile = Join-Path $repo 'craft-i18n.rev'
    if (Test-Path $revFile) {
      $i18n = Join-Path $DevDir 'craft-i18n'
      # Tinta Texto builds from the fork (upstream plus ca/gl/eu, wordcraft#10); fetch it by URL so
      # an older clone of storytold/craft-i18n works too.
      if (-not (Test-Path $i18n)) { git clone https://github.com/StDenis1980/craft-i18n $i18n; if ($LASTEXITCODE -ne 0) { throw 'craft-i18n clone failed' } }
      git -C $i18n fetch https://github.com/StDenis1980/craft-i18n '+refs/heads/*:refs/remotes/tinta/*'; if ($LASTEXITCODE -ne 0) { throw 'craft-i18n fetch failed' }
      git -C $i18n checkout --detach (Get-Content $revFile -Raw).Trim(); if ($LASTEXITCODE -ne 0) { throw 'craft-i18n checkout failed' }
      $env:CRAFT_I18N_DIR = $i18n; $env:CRAFT_I18N_REQUIRED = '1'
    }
    $dist = Join-Path $repo 'dist\release'
    Remove-Item -Recurse -Force $dist -ErrorAction SilentlyContinue
    # Through cmd with stderr merged: Windows PowerShell 5.1 turns a native tool's stderr into
    # terminating errors (cargo warnings). Paths under C:\dev have no spaces.
    $log = Join-Path $Logs "$app.log"
    cmd.exe /d /c "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $repo 'packaging\windows\package.ps1')`" -Arch x64 > `"$log`" 2>&1"
    if ($LASTEXITCODE -ne 0) { Get-Content $log -Tail 30; throw "${app}: package.ps1 failed (log: $log)" }
    Get-ChildItem $dist -Include '*.msi', '*-portable.zip' -Recurse | Copy-Item -Destination $Out
    if ($HasCycloneDx) {
      cargo cyclonedx --format json --override-filename "$app.cdx" | Out-Null
      Get-ChildItem $repo -Recurse -Filter "$app.cdx.json" -Depth 3 | Select-Object -First 1 | Copy-Item -Destination (Join-Path $Out 'sbom')
    }
  } finally { Pop-Location }
}

$sums = Get-ChildItem $Out -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object Name | ForEach-Object {
  "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)"
}
$sums | Set-Content -Encoding ascii (Join-Path $Out 'SHA256SUMS.txt')
Write-Output ''
Get-Content (Join-Path $Out 'SHA256SUMS.txt')
Write-Output ''
Write-Output 'Publish (pre-release, one per fork), after checking the installers:'
foreach ($app in $Apps) {
  $files = (Get-ChildItem $Out -File | Where-Object { $_.Name -like "*$((@{pdfcraft='tinta-pdf';wordcraft='tinta-texto';gridcraft='tinta-calculo';deckcraft='tinta-presenta';ibercorreo='tinta-correo'})[$app])*" } | ForEach-Object { "`"$($_.FullName)`"" }) -join ' '
  Write-Output "gh release create $Tag --repo StDenis1980/$app --prerelease --title `"$Tag`" --notes-file notes-$app.md $files `"$(Join-Path $Out 'SHA256SUMS.txt')`""
}
