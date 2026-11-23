# End-to-end smoke test for the CLI against a running relay.
#
# Usage: start the relay, then run this script. It launches two clients with
# the same mnemonic, waits for the handshake, sends a message from A, and
# checks that B received it.

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "target\debug\encchat.exe"
if (-not (Test-Path $exe)) { throw "Build the CLI first: cargo build -p encchat-cli" }

$mn = "desk alien potato grief invite festival tool century globe admit dress rare cram enforce nasty chronic occur cactus actor element case cattle present royal"
$server = if ($env:ENCCHAT_SERVER) { $env:ENCCHAT_SERVER } else { "ws://127.0.0.1:8787" }

function Start-Client($name) {
    $state = Join-Path $env:TEMP "encchat-smoke-$name"
    Remove-Item "$state.state", "$state.salt" -ErrorAction SilentlyContinue
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = "chat --server $server --mnemonic `"$mn`" --state `"$state`" --password-stdin"
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    [System.Diagnostics.Process]::Start($psi)
}

$A = Start-Client "a"
$B = Start-Client "b"

foreach ($p in @($A, $B)) {
    $p.StandardInput.WriteLine("pw")
    $p.StandardInput.WriteLine("pw")
}

Start-Sleep -Seconds 3
$A.StandardInput.WriteLine("hello from A")
Start-Sleep -Seconds 3

$A.StandardInput.WriteLine("/quit")
$B.StandardInput.WriteLine("/quit")

$A.WaitForExit(5000) | Out-Null
$B.WaitForExit(5000) | Out-Null

$outA = $A.StandardOutput.ReadToEnd()
$outB = $B.StandardOutput.ReadToEnd()
$errB = $B.StandardError.ReadToEnd()

Write-Output "----- client A -----"
Write-Output $outA
Write-Output "----- client B -----"
Write-Output $outB
if ($errB) { Write-Output "----- client B stderr -----"; Write-Output $errB }

if ($outB -match "hello from A") {
    Write-Output "SMOKE PASS: B received A's message over the encrypted relay"
} else {
    Write-Output "SMOKE FAIL: B did not receive the message"
    exit 1
}
