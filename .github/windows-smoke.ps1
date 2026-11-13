# Smoke test for `celld dev` on Windows, used by .github/workflows/windows.yml.
#
#   ./.github/windows-smoke.ps1 -Example hello -Expect 'regex'
#   ./.github/windows-smoke.ps1 -Example counter -Counter [-Restart]
#
# -Counter drives examples/counter, whose Durable Object is a SQLite cell named
# "Counter:<hex>" (a colon in a cell scope is the case the Windows path
# encoding exists for), and expects n to count 1, 2. -Restart then hard-kills
# the supervisor, expects the job object to have taken the node with it, starts
# again without --clean and expects n to continue at 3.
param(
    [Parameter(Mandatory = $true)][string]$Example,
    [string]$Expect = '',
    [switch]$Counter,
    [switch]$Restart
)
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$exe = Join-Path $root 'target\debug\celld.exe'
$dir = Join-Path $root "examples\$Example"
$port = 9876
$base = "http://127.0.0.1:$port"
$logs = Join-Path $env:RUNNER_TEMP "celld-$Example"
New-Item -ItemType Directory -Force -Path $logs | Out-Null

function Start-Dev([bool]$clean) {
    $arguments = @('dev', '--logs')
    if ($clean) { $arguments += '--clean' }
    $stamp = [guid]::NewGuid().ToString('N').Substring(0, 8)
    $process = Start-Process -FilePath $exe -ArgumentList $arguments -WorkingDirectory $dir `
        -PassThru -NoNewWindow `
        -RedirectStandardOutput (Join-Path $logs "out-$stamp.log") `
        -RedirectStandardError (Join-Path $logs "err-$stamp.log")
    return $process
}

function Wait-Ready([int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $response = Invoke-WebRequest -Uri "$base/" -UseBasicParsing -TimeoutSec 5
            if ($response.StatusCode -eq 200) { return $response }
        } catch { Start-Sleep -Milliseconds 500 }
    }
    throw "celld dev did not answer on $base within $seconds s"
}

function Show-Logs {
    Get-ChildItem $logs | ForEach-Object {
        Write-Host "---- $($_.Name)"
        Get-Content $_.FullName -Tail 80
    }
}

function Get-Count {
    $response = Invoke-WebRequest -Uri "$base/?name=smoke" -UseBasicParsing -TimeoutSec 30
    return [int]($response.Content | ConvertFrom-Json).n
}

try {
    $supervisor = Start-Dev $true
    $ready = Wait-Ready 180
    if ($Counter) {
        $first = Get-Count
        $second = Get-Count
        if ($first -ne 1 -or $second -ne 2) { throw "counter gave $first then $second, expected 1 then 2" }
        Write-Host "counter: $first, $second"
    } elseif ($Expect -ne '' -and $ready.Content -notmatch $Expect) {
        throw "response did not match /$Expect/: $($ready.Content)"
    } else {
        Write-Host "hello: $($ready.Content)"
    }

    if ($Restart) {
        Stop-Process -Id $supervisor.Id -Force
        Start-Sleep -Seconds 5
        $left = @(Get-Process -Name celld -ErrorAction SilentlyContinue)
        if ($left.Count -gt 0) {
            $left | Stop-Process -Force
            throw "the node outlived its supervisor ($($left.Count) celld process(es)); the job object did not apply"
        }
        $supervisor = Start-Dev $false
        Wait-Ready 180 | Out-Null
        $third = Get-Count
        if ($third -ne 3) { throw "counter gave $third after the restart, expected 3" }
        Write-Host "counter after hard kill: $third"
    }
} catch {
    Show-Logs
    throw
} finally {
    if ($supervisor -and -not $supervisor.HasExited) { Stop-Process -Id $supervisor.Id -Force }
    Start-Sleep -Seconds 1
    Get-Process -Name celld -ErrorAction SilentlyContinue | Stop-Process -Force
}
