<#
.SYNOPSIS
  Tinta Office: smoke-test an installed Tinta PDF before a preview goes out.
.DESCRIPTION
  Runs the installed pdfcraft-cli.exe on files it makes itself, in a temporary folder, and checks
  that each of the things testers will try first works:
    - the version starts;
    - a PDF is made from text and drawn as an image;
    - OCR reads Spanish (with n-tilde) from that image;
    - a Word, an Excel and a PowerPoint file become PDFs through the installed Tinta Texto,
      Calculo and Presenta (each skipped, and said, when that app is not installed), and Combine
      joins them with a PDF;
    - a document is signed with a throwaway self-signed ID and its signature checks out.
  Nothing outside the temporary folder is changed, no certificate store is touched and nothing
  goes over the network. TINTA_*_CLI variables are cleared for the run, so the apps must be found
  the way a user's copy finds them.
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\windows\tinta-smoke.ps1
.EXAMPLE
  powershell -File tinta-smoke.ps1 -Cli 'D:\portable\Tinta PDF\pdfcraft-cli.exe' -Keep
  Portable builds side by side (D:\portable\Tinta PDF, D:\portable\Tinta Texto, ...): the sample
  Office files come from the apps in D:\portable, as -Suite defaults to the folder above -Cli's.
.EXAMPLE
  powershell -File tinta-smoke.ps1 -Cli 'D:\rc\pdf\pdfcraft-cli.exe' -Suite 'D:\rc\suite'
#>
param(
  # The pdfcraft-cli.exe to test (default: the installed one).
  [string] $Cli = (Join-Path ${env:ProgramFiles} 'Tinta Office\Tinta PDF\pdfcraft-cli.exe'),
  # The folder holding the 'Tinta Texto', 'Tinta Calculo' and 'Tinta Presenta' folders whose CLIs
  # make the sample Office files (default: the folder above -Cli's, where pdfcraft-cli itself
  # looks for its sibling apps; for the installed copy that is Program Files\Tinta Office).
  [string] $Suite = '',
  # Keep the temporary folder (with every file made) for a look afterwards.
  [switch] $Keep
)
$ErrorActionPreference = 'Stop'
if (-not $Suite) {
  $Suite = Split-Path -Parent (Split-Path -Parent $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Cli))
}
$results = New-Object System.Collections.Generic.List[object]
function Pass([string] $What) { $results.Add([pscustomobject]@{ Check = $What; Result = 'ok' }) }
function Fail([string] $What, [string] $Why) { $results.Add([pscustomobject]@{ Check = $What; Result = "FAILED: $Why" }) }
function Skip([string] $What, [string] $Why) { $results.Add([pscustomobject]@{ Check = $What; Result = "skipped: $Why" }) }

# Run a program with its output in files (Windows PowerShell 5.1 would turn its stderr into
# errors and read its UTF-8 output in the console's code page).
function Invoke-Tool([string] $Exe, [string[]] $Arguments) {
  $out = Join-Path $work 'stdout.txt'
  $err = Join-Path $work 'stderr.txt'
  $quoted = $Arguments | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }
  $p = Start-Process -FilePath $Exe -ArgumentList $quoted -WorkingDirectory $work -NoNewWindow -Wait -PassThru `
    -RedirectStandardOutput $out -RedirectStandardError $err
  [pscustomobject]@{
    Code = $p.ExitCode
    Out = [IO.File]::ReadAllText($out, [Text.Encoding]::UTF8)
    Err = [IO.File]::ReadAllText($err, [Text.Encoding]::UTF8).Trim()
  }
}

function Write-Utf8([string] $Path, [string] $Text) {
  [IO.File]::WriteAllText($Path, $Text, (New-Object Text.UTF8Encoding($false)))
}

if (-not (Test-Path -LiteralPath $Cli)) { throw "pdfcraft-cli not found at $Cli (pass -Cli)" }
$work = Join-Path ([IO.Path]::GetTempPath()) ("tinta-smoke-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
foreach ($v in 'TINTA_TEXTO_CLI', 'TINTA_CALCULO_CLI', 'TINTA_PRESENTA_CLI') { Remove-Item "env:$v" -ErrorAction SilentlyContinue }

try {
  # 1. The version.
  $r = Invoke-Tool $Cli @('--version')
  if ($r.Code -eq 0 -and $r.Out -match '\d+\.\d+\.\d+') { Pass "version $($Matches[0])" } else { Fail 'version' $r.Err }

  # 2. A PDF from text, drawn as a 200 dpi picture: the "scan" OCR reads next.
  $nTilde = [char]0x00F1
  Write-Utf8 (Join-Path $work 'texto.json') ('[{"tool":"doc_create","args":{"from":"text","text":"Factura de la luz\nSe\u00f1or Mu\u00f1oz, a\u00f1o 2026, importe 120,50 euros."}},' +
    '{"tool":"doc_save","args":{"doc":1,"path":"texto.pdf"}}]')
  $r = Invoke-Tool $Cli @('run', '--script', 'texto.json', '--root', $work)
  if ($r.Code -eq 0) { Pass 'PDF made from text' } else { Fail 'PDF made from text' $r.Err }
  $r = Invoke-Tool $Cli @('render', 'texto.pdf', '--page', '1', '--dpi', '200', '--out', 'scan.png')
  if ($r.Code -eq 0) { Pass 'page drawn as an image' } else { Fail 'page drawn as an image' $r.Err }

  # 3. Office files from the installed suite apps' own samples.
  $office = @()
  $makers = @(
    @{ App = 'Tinta Texto'; Exe = 'wordcraft-cli.exe'; File = 'carta.docx'; Args = @('run', '--template', 'sample', '--save', 'carta.docx') },
    @{ App = 'Tinta Calculo'; Exe = 'gridcraft-cli.exe'; File = 'hoja.xlsx'; Args = @('run', '--sample', 'sales', '--out', 'hoja.xlsx', '--quiet') },
    @{ App = 'Tinta Presenta'; Exe = 'deckcraft-cli.exe'; File = 'deck.pptx'; Args = @('run', '--sample', '--save', 'deck.pptx') }
  )
  foreach ($m in $makers) {
    $exe = Join-Path (Join-Path $Suite $m.App) $m.Exe
    if (-not (Test-Path -LiteralPath $exe)) { Skip "$($m.App): $($m.File) to PDF" "$($m.App) is not installed"; continue }
    $r = Invoke-Tool $exe $m.Args
    if ($r.Code -eq 0 -and (Test-Path -LiteralPath (Join-Path $work $m.File))) { $office += $m } else { Fail "$($m.App) sample $($m.File)" $r.Err }
  }

  # 4. Two runs of the automation tools, so a failure in one doesn't hide the other: OCR, the
  # Office files and Combine; then a signature.
  function Invoke-Steps([string] $Name, [string[]] $Steps) {
    Write-Utf8 (Join-Path $work "$Name.json") ('[' + ($Steps -join ",`n") + ']')
    $r = Invoke-Tool $Cli @('run', '--script', "$Name.json", '--root', $work, '--report')
    $byStep = @{}
    try { foreach ($s in ($r.Out | ConvertFrom-Json).steps) { $byStep[[int] $s.step] = $s } } catch { Fail "automation run $Name" "no report: $($r.Err)" }
    return $byStep
  }
  function Step-Result($Run, [int] $n) { $s = $Run[$n]; if ($s -and $s.status -eq 'ok') { return @($s.result)[0] } return $null }
  function Step-Error($Run, [int] $n) { $s = $Run[$n]; if ($s -and $s.error) { return "$($s.error)" } return 'not run: an earlier step failed' }

  $steps = New-Object System.Collections.Generic.List[string]
  $steps.Add('{"tool":"doc_create","args":{"from":"images","paths":["scan.png"]}}')
  $steps.Add('{"tool":"ocr_recognize","args":{"doc":1,"language":"es"}}')
  $steps.Add('{"tool":"text_find","args":{"doc":1,"query":"Mu\u00f1oz"}}')
  foreach ($m in $office) { $steps.Add('{"tool":"doc_create","args":{"from":"office","path":"' + $m.File + '"}}') }
  if ($office.Count -gt 0) {
    $paths = (@('"texto.pdf"') + ($office | ForEach-Object { '"' + $_.File + '"' })) -join ','
    $steps.Add('{"tool":"doc_combine","args":{"paths":[' + $paths + '],"out":"todo.pdf"}}')
  }
  $run = Invoke-Steps 'convert' $steps
  $ocr = Step-Result $run 2
  $found = Step-Result $run 3
  if ($ocr -and $found -and $found.count -ge 1) { Pass "OCR read Spanish ($($ocr.words) words, Mu${nTilde}oz found)" }
  else { Fail 'OCR in Spanish' $(if ($ocr) { "read: $(@($ocr.pages)[0].text)" } else { Step-Error $run 2 }) }
  $n = 4
  foreach ($m in $office) {
    $made = Step-Result $run $n
    if ($made -and $made.pages -ge 1) { Pass "$($m.App): $($m.File) to PDF ($($made.pages) pages)" } else { Fail "$($m.App): $($m.File) to PDF" (Step-Error $run $n) }
    $n++
  }
  if ($office.Count -gt 0) {
    if (Step-Result $run $n) { Pass "Combine: a PDF and $($office.Count) Office file(s) in one" } else { Fail 'Combine with Office files' (Step-Error $run $n) }
  }

  $run = Invoke-Steps 'sign' @(
    '{"tool":"sign_id_create","args":{"name":"Tinta Smoke Test","password":"smoke-test-1","path":"prueba.p12"}}',
    '{"tool":"doc_open","args":{"path":"texto.pdf"}}',
    '{"tool":"sign_document","args":{"doc":1,"id":"prueba.p12","password":"smoke-test-1","out":"firmado.pdf","page":1,"rect":[50,50,250,120]}}',
    '{"tool":"doc_open","args":{"path":"firmado.pdf"}}',
    '{"tool":"sign_list","args":{"doc":2}}'
  )
  $list = Step-Result $run 5
  $sig = if ($list) { @($list.signatures)[0] } else { $null }
  # A self-signed ID is not trusted, so "unknown" (intact, signer not trusted) is the right answer.
  if ($sig -and ($sig.status -eq 'valid' -or $sig.status -eq 'unknown')) { Pass "signed and checked ($($sig.status): intact, test ID not trusted)" }
  else {
    $why = if ($sig) { "status $($sig.status)" } else { @(1..5 | Where-Object { -not (Step-Result $run $_) } | ForEach-Object { Step-Error $run $_ })[0] }
    Fail 'signing' $why
  }
} finally {
  $results | Format-Table -AutoSize -Wrap | Out-String -Width 160 | Write-Host
  if ($Keep) { Write-Host "Files kept in $work" } else { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }
}
$failed = @($results | Where-Object { $_.Result -like 'FAILED*' }).Count
if ($failed -gt 0) { Write-Host "$failed check(s) failed."; exit 1 }
Write-Host 'All checks passed.'
