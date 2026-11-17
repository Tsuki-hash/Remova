[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^[a-z0-9_.-]+/[a-z0-9_.-]+$')][string]$Repository,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit,
    [ValidateRange(0, 45)][int]$WaitMinutes = 0
)

$ErrorActionPreference = 'Stop'

function Invoke-CiApi([string[]]$Arguments) {
    $result = gh api @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'GitHub API call failed while checking release CI.' }
    return $result
}

function Assert-OnMain {
    $comparison = Invoke-CiApi @("repos/$Repository/compare/main...$Commit", '--jq', '.status')
    if ($comparison -cnotin @('behind', 'identical')) {
        throw "Release commit $Commit is not on main."
    }
}

function Get-LatestRun {
    $query = "repos/$Repository/actions/workflows/ci.yml/runs?head_sha=$Commit&branch=main&event=push&per_page=100"
    $pages = Invoke-CiApi @($query, '--paginate', '--slurp') | Out-String | ConvertFrom-Json -AsHashtable -NoEnumerate
    if ($pages -isnot [array] -or $pages.Count -eq 0) {
        throw 'GitHub did not return CI runs.'
    }
    $runs = @(foreach ($page in $pages) {
        if ($page.workflow_runs -isnot [array]) { throw 'GitHub returned an invalid CI run list.' }
        foreach ($run in $page.workflow_runs) {
            if ($run.head_sha -cne $Commit -or $run.head_branch -cne 'main' -or $run.event -cne 'push' -or
                $run.id -notmatch '^[1-9]\d*$' -or $run.run_number -notmatch '^[1-9]\d*$' -or
                $run.run_attempt -notmatch '^[1-9]\d*$' -or !$run.status) {
                throw 'GitHub returned a CI run that does not match the release commit and main push.'
            }
            $run
        }
    })
    $runs | Sort-Object { [long]$_.run_number } -Descending | Select-Object -First 1
}

Assert-OnMain
$deadline = [DateTime]::UtcNow.AddMinutes($WaitMinutes)
$registration = [DateTime]::UtcNow.AddMinutes([Math]::Min(2, $WaitMinutes))
while ($true) {
    $run = Get-LatestRun
    if (!$run) {
        if ([DateTime]::UtcNow -ge $registration) {
            throw "No main push CI run exists for $Commit."
        }
        Start-Sleep -Seconds 10
        continue
    }
    if ($run.status -ceq 'completed') { break }
    if ([DateTime]::UtcNow -ge $deadline) {
        throw "CI run $($run.id) is still '$($run.status)' for $Commit."
    }
    Start-Sleep -Seconds 20
}
if ($run.conclusion -cne 'success') {
    throw "CI run $($run.id) concluded '$($run.conclusion)' for $Commit."
}

$jobsQuery = "repos/$Repository/actions/runs/$($run.id)/jobs?filter=latest&per_page=100"
$jobPages = Invoke-CiApi @($jobsQuery, '--paginate', '--slurp') | Out-String | ConvertFrom-Json -AsHashtable -NoEnumerate
if ($jobPages -isnot [array] -or $jobPages.Count -eq 0) {
    throw 'GitHub did not return the release CI jobs.'
}
$jobs = @(foreach ($page in $jobPages) {
    if ($page.jobs -isnot [array]) { throw 'GitHub returned an invalid CI job list.' }
    $page.jobs
})
foreach ($required in @('test', 'package')) {
    $matched = @($jobs | Where-Object { $_.name -ceq $required })
    if ($matched.Count -ne 1 -or $matched[0].run_id -ne $run.id -or
        $matched[0].head_sha -cne $Commit -or $matched[0].status -cne 'completed' -or
        $matched[0].conclusion -cne 'success') {
        throw "CI job '$required' must have one successful result for run $($run.id) and commit $Commit."
    }
}

# Refuse to combine the conclusion and jobs from different reruns.
$current = Get-LatestRun
if (!$current -or $current.id -ne $run.id -or $current.run_attempt -ne $run.run_attempt -or
    $current.status -cne 'completed' -or $current.conclusion -cne 'success') {
    throw "CI changed during verification for $Commit."
}
Assert-OnMain
Write-Host "Verified main CI run $($run.id), attempt $($run.run_attempt), for $Commit (test + package)."
