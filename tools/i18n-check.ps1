# Validate the gettext catalog: duplicate msgid/context pairs and placeholder parity.
# Dynamic Rust strings are validated by cargo test (i18n catalog test); this
# script covers the static @tr catalog used by Slint.
$ErrorActionPreference = "Stop"
$path = Join-Path $PSScriptRoot "..\crates\slint-ui\lang\en_US\LC_MESSAGES\flint-slint.po"
$entries = [System.Collections.Generic.Dictionary[string,string]]::new([StringComparer]::Ordinal)
$errors = @()
$msgctxt = ""
$msgid = ""
$msgstr = ""
$mode = ""
function Commit {
    if ($script:msgid -ne "") {
        $key = "$script:msgctxt|$script:msgid"
        if ($script:entries.ContainsKey($key)) {
            $script:errors += "duplicate catalog key: $key"
        } else {
            $script:entries[$key] = $script:msgstr
        }
        $sp = { param($s) ([regex]::Matches($s, '\{\w*\}|%\d*\$?[sd]')).Value -join "," }
        if ((& $sp $script:msgid) -ne (& $sp $script:msgstr)) {
            $script:errors += "placeholder mismatch: $key"
        }
    }
    $script:msgctxt = ""; $script:msgid = ""; $script:msgstr = ""; $script:mode = ""
}
foreach ($line in Get-Content $path) {
    if ($line -match '^msgctxt\s+"(.*)"') { $msgctxt = $Matches[1]; $mode = "ctxt" }
    elseif ($line -match '^msgid\s+"(.*)"') { $msgid = $Matches[1]; $mode = "id" }
    elseif ($line -match '^msgstr\s+"(.*)"') { $msgstr = $Matches[1]; $mode = "str" }
    elseif ($line -match '^"(.*)"') {
        if ($mode -eq "ctxt") { $msgctxt += $Matches[1] }
        elseif ($mode -eq "id") { $msgid += $Matches[1] }
        elseif ($mode -eq "str") { $msgstr += $Matches[1] }
    }
    elseif ($line.Trim() -eq "") { Commit }
}
Commit
if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Host "FAIL: $_" }
    exit 1
}
Write-Host "i18n: ok ($($entries.Count) static entries)"
