#!/usr/bin/env bash
# Tinta Office: smoke-test an installed Tinta PDF before a preview goes out (the Linux twin of
# packaging/windows/tinta-smoke.ps1).
#
# Runs the installed pdfcraft-cli on files it makes itself, in a temporary folder, and checks that
# the version starts; a PDF is made from text and drawn as an image; OCR reads Spanish (with ñ)
# from it; a Word, an Excel and a PowerPoint file become PDFs through the installed Tinta Texto,
# Calculo and Presenta (each skipped, and said, when that app is not installed) and Combine joins
# them with a PDF; a document is signed with a throwaway self-signed ID and its signature checks
# out. Nothing outside the temporary folder changes and nothing goes over the network.
# TINTA_*_CLI variables are cleared, so the apps must be found as a user's copy finds them.
#
# Usage: packaging/linux/tinta-smoke.sh [path/to/pdfcraft-cli] [--keep]
# Needs python3 (to read the JSON report).
set -u

CLI=pdfcraft-cli
KEEP=0
for arg in "$@"; do
    case "$arg" in
        --keep) KEEP=1 ;;
        *) CLI=$arg ;;
    esac
done
command -v "$CLI" >/dev/null 2>&1 || { echo "pdfcraft-cli not found: $CLI" >&2; exit 2; }
command -v python3 >/dev/null 2>&1 || { echo "python3 is needed to read the report" >&2; exit 2; }
CLI=$(command -v "$CLI")
unset TINTA_TEXTO_CLI TINTA_CALCULO_CLI TINTA_PRESENTA_CLI

WORK=$(mktemp -d "${TMPDIR:-/tmp}/tinta-smoke-XXXXXXXX")
RESULTS="$WORK/results.txt"
: >"$RESULTS"
pass() { printf 'ok       %s\n' "$1" >>"$RESULTS"; }
fail() { printf 'FAILED   %s: %s\n' "$1" "$2" >>"$RESULTS"; }
skip() { printf 'skipped  %s: %s\n' "$1" "$2" >>"$RESULTS"; }
finish() {
    cat "$RESULTS"
    if grep -q '^FAILED' "$RESULTS"; then echo "$(grep -c '^FAILED' "$RESULTS") check(s) failed."; else echo "All checks passed."; fi
    if [ "$KEEP" = 1 ]; then echo "Files kept in $WORK"; else rm -rf "$WORK"; fi
}
trap finish EXIT
cd "$WORK" || exit 2

# 1. The version.
if v=$("$CLI" --version 2>err.txt) && [[ $v =~ [0-9]+\.[0-9]+\.[0-9]+ ]]; then pass "version ${BASH_REMATCH[0]}"; else fail version "$(cat err.txt)"; fi

# 2. A PDF from text, drawn as a 200 dpi picture: the "scan" OCR reads next.
cat >texto.json <<'EOF'
[{"tool":"doc_create","args":{"from":"text","text":"Factura de la luz\nSeñor Muñoz, año 2026, importe 120,50 euros."}},
 {"tool":"doc_save","args":{"doc":1,"path":"texto.pdf"}}]
EOF
if "$CLI" run --script texto.json --root "$WORK" >/dev/null 2>err.txt; then pass "PDF made from text"; else fail "PDF made from text" "$(cat err.txt)"; fi
if "$CLI" render texto.pdf --page 1 --dpi 200 --out scan.png >/dev/null 2>err.txt; then pass "page drawn as an image"; else fail "page drawn as an image" "$(cat err.txt)"; fi

# 3. Office files from the installed suite apps' own samples.
OFFICE=()
make_office() { # app, tool, file, args…
    local app=$1 tool=$2 file=$3
    shift 3
    if ! command -v "$tool" >/dev/null 2>&1; then skip "$app: $file to PDF" "$app is not installed"; return; fi
    if "$tool" "$@" >/dev/null 2>err.txt && [ -f "$file" ]; then OFFICE+=("$app|$file"); else fail "$app sample $file" "$(cat err.txt)"; fi
}
make_office "Tinta Texto" wordcraft-cli carta.docx run --template sample --save carta.docx
make_office "Tinta Calculo" gridcraft-cli hoja.xlsx run --sample sales --out hoja.xlsx --quiet
make_office "Tinta Presenta" deckcraft-cli deck.pptx run --sample --save deck.pptx

# 4. Two runs of the automation tools, so a failure in one doesn't hide the other: OCR, the
# Office files and Combine; then a signature.
{
    echo '[{"tool":"doc_create","args":{"from":"images","paths":["scan.png"]}},'
    echo ' {"tool":"ocr_recognize","args":{"doc":1,"language":"es"}},'
    printf '%s' ' {"tool":"text_find","args":{"doc":1,"query":"Muñoz"}}'
    paths='"texto.pdf"'
    for entry in ${OFFICE[@]+"${OFFICE[@]}"}; do
        file=${entry#*|}
        printf ',\n {"tool":"doc_create","args":{"from":"office","path":"%s"}}' "$file"
        paths="$paths,\"$file\""
    done
    if [ ${#OFFICE[@]} -gt 0 ]; then printf ',\n {"tool":"doc_combine","args":{"paths":[%s],"out":"todo.pdf"}}' "$paths"; fi
    echo ']'
} >convert.json
cat >sign.json <<'JSON'
[{"tool":"sign_id_create","args":{"name":"Tinta Smoke Test","password":"smoke-test-1","path":"prueba.p12"}},
 {"tool":"doc_open","args":{"path":"texto.pdf"}},
 {"tool":"sign_document","args":{"doc":1,"id":"prueba.p12","password":"smoke-test-1","out":"firmado.pdf","page":1,"rect":[50,50,250,120]}},
 {"tool":"doc_open","args":{"path":"firmado.pdf"}},
 {"tool":"sign_list","args":{"doc":2}}]
JSON
"$CLI" run --script convert.json --root "$WORK" --report >convert-report.json 2>err.txt
"$CLI" run --script sign.json --root "$WORK" --report >sign-report.json 2>err.txt
printf '%s\n' ${OFFICE[@]+"${OFFICE[@]}"} >office.txt

python3 -I - convert-report.json sign-report.json office.txt "$RESULTS" <<'PY'
import json, sys
convert_path, sign_path, office_path, results_path = sys.argv[1:5]
out = open(results_path, "a", encoding="utf-8")
def line(state, what, why=""):
    out.write(f"{state:<8} {what}" + (f": {why}" if why else "") + "\n")
def load(path, name):
    try:
        return {s.get("step"): s for s in json.load(open(path, encoding="utf-8")).get("steps", [])}
    except Exception as e:
        line("FAILED", f"automation run {name}", f"no report ({e})")
        return {}
def result(run, n):
    s = run.get(n)
    if s and s.get("status") == "ok":
        r = s.get("result")
        return r[0] if isinstance(r, list) and r else r
    return None
def error(run, n):
    s = run.get(n)
    return str(s["error"]) if s and s.get("error") else "not run: an earlier step failed"
run = load(convert_path, "convert")
office = [l.strip().split("|", 1) for l in open(office_path, encoding="utf-8") if l.strip()]
ocr, found = result(run, 2), result(run, 3)
if ocr and found and found.get("count", 0) >= 1:
    line("ok", f"OCR read Spanish ({ocr.get('words')} words, Muñoz found)")
else:
    line("FAILED", "OCR in Spanish", f"read: {ocr['pages'][0]['text']}" if ocr else error(run, 2))
n = 4
for app, file in office:
    made = result(run, n)
    if made and made.get("pages", 0) >= 1:
        line("ok", f"{app}: {file} to PDF ({made['pages']} pages)")
    else:
        line("FAILED", f"{app}: {file} to PDF", error(run, n))
    n += 1
if office:
    if result(run, n):
        line("ok", f"Combine: a PDF and {len(office)} Office file(s) in one")
    else:
        line("FAILED", "Combine with Office files", error(run, n))
run = load(sign_path, "sign")
sig = (result(run, 5) or {}).get("signatures", [None])[0]
# A self-signed ID is not trusted, so "unknown" (intact, signer not trusted) is the right answer.
if sig and sig.get("status") in ("valid", "unknown"):
    line("ok", f"signed and checked ({sig['status']}: intact, test ID not trusted)")
else:
    first = next((error(run, i) for i in range(1, 6) if result(run, i) is None), "no signature listed")
    line("FAILED", "signing", f"status {sig.get('status')}" if sig else first)
PY

if grep -q '^FAILED' "$RESULTS"; then exit 1; fi
