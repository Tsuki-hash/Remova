$ErrorActionPreference = 'Stop'
$tag = $env:RELEASE_TAG
$repository = $env:GITHUB_REPOSITORY
$actor = $env:DISPATCH_ACTOR
if ($tag -cnotmatch '^v\d+\.\d+\.\d+$' -or
    $repository -notmatch '^[a-z0-9_.-]+/[a-z0-9_.-]+$' -or
    $actor -notmatch '^[a-z0-9-]+$') {
    throw 'Invalid release tag, repository or maintainer.'
}

function Invoke-GitHub([string[]]$Arguments) {
    $result = gh api @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'GitHub API call failed.' }
    return $result
}

$permission = Invoke-GitHub @("repos/$repository/collaborators/$actor/permission", '--jq', '.role_name')
if ($permission -cnotin @('admin', 'maintain')) {
    throw 'Publishing a release requires a repository maintainer or administrator.'
}

function Get-TagCommit {
    $commit = Invoke-GitHub @("repos/$repository/commits/$tag", '--jq', '.sha')
    if ($commit -cnotmatch '^[a-f0-9]{40}$') { throw 'The release tag does not resolve to a commit.' }
    return $commit
}

function Get-Draft {
    $pages = Invoke-GitHub @("repos/$repository/releases?per_page=100", '--paginate', '--slurp') | ConvertFrom-Json
    $matches = @(foreach ($page in $pages) {
        $page | Where-Object { $_.tag_name -ceq $tag }
    })
    if ($matches.Count -ne 1 -or !$matches[0].draft -or !$matches[0].id) {
        throw 'The tag must have an unpublished release draft.'
    }
    return $matches[0]
}

function Get-AttachmentIdentity($Draft) {
    $Draft.assets | Sort-Object id | Select-Object id, name, digest, state | ConvertTo-Json -Compress
}

$commit = Get-TagCommit
$draft = Get-Draft
$identity = Get-AttachmentIdentity $draft
$download = Join-Path $env:RUNNER_TEMP "reviewed-release-$($draft.id)"
if (Test-Path -LiteralPath $download) { throw 'The draft download directory already exists.' }
New-Item -ItemType Directory -Path $download | Out-Null
gh release download $tag --repo $repository --pattern '*' --dir $download
if ($LASTEXITCODE -ne 0) { throw 'Could not download the draft attachments.' }

& "$PSScriptRoot/release-approval.ps1" -Tag $tag -Commit $commit -BundleDir $download -ApprovalJson $env:APPROVAL_JSON
$current = Get-Draft
if ($current.id -ne $draft.id -or (Get-AttachmentIdentity $current) -cne $identity -or (Get-TagCommit) -cne $commit) {
    throw 'The release draft or tag changed during verification.'
}
Invoke-GitHub @('--method', 'PATCH', "repos/$repository/releases/$($draft.id)", '-F', 'draft=false', '--silent')
Write-Host "Published verified release $tag."
