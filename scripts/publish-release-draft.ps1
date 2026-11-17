$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/release-notes-policy.ps1"
$tag = $env:RELEASE_TAG
$repository = $env:GITHUB_REPOSITORY
$actor = $env:DISPATCH_ACTOR
if ($tag -cnotmatch '^v\d+\.\d+\.\d+$' -or
    $repository -notmatch '^[a-z0-9_.-]+/[a-z0-9_.-]+$' -or
    $actor -notmatch '^[a-z0-9-]+$') {
    throw 'Invalid release tag, repository or maintainer.'
}
if ([string]::IsNullOrWhiteSpace($env:RELEASE_APPROVAL_PUBLIC_KEY)) {
    throw 'Configure the RELEASE_APPROVAL_PUBLIC_KEY repository variable before publishing.'
}
$version = $tag.Substring(1)

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
    $commit = Invoke-GitHub @("repos/$repository/commits/refs/tags/$tag", '--jq', '.sha')
    if ($commit -cnotmatch '^[a-f0-9]{40}$') { throw 'The release tag does not resolve to a commit.' }
    return $commit
}

function Get-Draft {
    $pages = Invoke-GitHub @("repos/$repository/releases?per_page=100", '--paginate', '--slurp') | ConvertFrom-Json
    $matches = @(foreach ($page in $pages) {
        $page | Where-Object { $_.tag_name -ceq $tag }
    })
    if ($matches.Count -ne 1 -or $matches[0].draft -isnot [bool] -or !$matches[0].draft -or !$matches[0].id) {
        throw 'The tag must have an unpublished release draft.'
    }
    return $matches[0]
}

function Get-AttachmentIdentity($Draft) {
    ConvertTo-Json -InputObject @($Draft.assets | Sort-Object id | Select-Object id, name, digest, state) -Compress
}

function Assert-DownloadedAttachments($Draft, [string]$Directory) {
    $files = @(Get-ChildItem -LiteralPath $Directory -File)
    if ($files.Count -ne @($Draft.assets).Count) {
        throw 'The downloaded attachments do not match the draft inventory.'
    }
    foreach ($file in $files) {
        $remote = @($Draft.assets | Where-Object { $_.name -ceq $file.Name })
        $sha = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($remote.Count -ne 1 -or $remote[0].state -cne 'uploaded' -or
            $remote[0].id -notmatch '^[1-9]\d*$' -or $remote[0].digest -cne "sha256:$sha") {
            throw "The GitHub attachment does not match the downloaded $($file.Name)."
        }
    }
}

function Test-PublicationMetadata($Draft, $Approved) {
    if ($Draft.name -cne $Approved.name -or $Draft.body -isnot [string] -or
        $Draft.prerelease -isnot [bool] -or $Draft.prerelease -ne $Approved.prerelease) {
        return $false
    }
    $bodyBytes = [Text.Encoding]::UTF8.GetBytes($Draft.body)
    $bodyHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bodyBytes)).ToLowerInvariant()
    return $bodyHash -ceq $Approved.body_sha256
}

function Get-LatestRelease {
    $response = gh api "repos/$repository/releases/latest" --include 2>&1 | Out-String
    $code = $LASTEXITCODE
    if ($response -notmatch '(?m)^HTTP/[\d.]+\s+(\d{3})') { throw 'Missing latest release response status.' }
    $status = [int]$Matches[1]
    if ($status -eq 404) { return $null }
    if ($code -ne 0 -or $status -ne 200) { throw 'Could not read the latest release.' }
    $latest = ($response -split '\r?\n\r?\n', 2)[1] | ConvertFrom-Json
    if (!$latest.id -or !$latest.tag_name) { throw 'Invalid latest release identity.' }
    return @{ id = $latest.id; tag = $latest.tag_name }
}

$commit = Get-TagCommit
& "$PSScriptRoot/require-release-ci.ps1" -Repository $repository -Commit $commit
$draft = Get-Draft
$identity = Get-AttachmentIdentity $draft
$download = Join-Path $env:RUNNER_TEMP "reviewed-release-$($draft.id)"
if (Test-Path -LiteralPath $download) { throw 'The draft download directory already exists.' }
New-Item -ItemType Directory -Path $download | Out-Null
gh release download $tag --repo $repository --pattern '*' --dir $download
if ($LASTEXITCODE -ne 0) { throw 'Could not download the draft attachments.' }

$publication = & "$PSScriptRoot/release-approval.ps1" -Tag $tag -Commit $commit -BundleDir $download `
    -ApprovalJson $env:APPROVAL_JSON -PublicKeyPem $env:RELEASE_APPROVAL_PUBLIC_KEY
if (!(Test-PublicationMetadata $draft $publication)) { throw 'The draft metadata does not match the signed approval.' }
$unsigned = Test-UnsignedInstallers -BundleDir $download -Version $version
Assert-ReleaseDownloadNotes -Body $draft.body -Version $version -Unsigned:$unsigned
Assert-DownloadedAttachments $draft $download
& "$PSScriptRoot/require-release-ci.ps1" -Repository $repository -Commit $commit
$current = Get-Draft
if ($current.id -ne $draft.id -or (Get-AttachmentIdentity $current) -cne $identity -or
    !(Test-PublicationMetadata $current $publication) -or (Get-TagCommit) -cne $commit) {
    throw 'The release draft or tag changed during verification.'
}
# Send the approved values in the publish request itself, so a concurrent edit
# to the title, notes or release flags cannot become the published text.
$previousLatest = if ($publication.make_latest -ceq 'false') { Get-LatestRelease } else { $null }
$request = Join-Path $download 'publish-request.json'
@{
    name = $publication.name
    body = $draft.body
    prerelease = $publication.prerelease
    make_latest = $publication.make_latest
    draft = $false
} | ConvertTo-Json -Compress | Set-Content -LiteralPath $request -Encoding utf8NoBOM
Invoke-GitHub @('--method', 'PATCH', "repos/$repository/releases/$($draft.id)", '--input', $request, '--silent')
try {
    $published = Invoke-GitHub @("repos/$repository/releases/tags/$tag") | Out-String | ConvertFrom-Json
    if ($published.id -ne $draft.id -or $published.tag_name -cne $tag -or
        $published.draft -isnot [bool] -or $published.draft -or !$published.published_at -or
        !(Test-PublicationMetadata $published $publication) -or
        (Get-AttachmentIdentity $published) -cne $identity -or (Get-TagCommit) -cne $commit) {
        throw 'The public release no longer matches the approved tag, notes or attachments.'
    }
    $expectedLatest = if ($publication.make_latest -ceq 'true') { @{ id = $draft.id; tag = $tag } } else { $previousLatest }
    $latestMatches = $false
    for ($attempt = 0; $attempt -lt 3; $attempt++) {
        $latest = Get-LatestRelease
        $latestMatches = if ($null -eq $expectedLatest) { $null -eq $latest } else {
            $latest.id -eq $expectedLatest.id -and $latest.tag -ceq $expectedLatest.tag
        }
        if ($latestMatches) { break }
        if ($attempt -lt 2) { Start-Sleep -Milliseconds 500 }
    }
    if (!$latestMatches) { throw 'The latest release does not match the approved publication intent.' }
    @{
        release_id = $published.id
        tag = $tag
        commit = $commit
        name = $published.name
        body_sha256 = $publication.body_sha256
        prerelease = $published.prerelease
        make_latest = $publication.make_latest
        previous_latest = $previousLatest
        verified_latest = $latest
        published_at = $published.published_at
        verified_at = [DateTime]::UtcNow.ToString('o')
        assets = @($published.assets | Select-Object id, name, digest)
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $env:RUNNER_TEMP 'release-publication.json') -Encoding utf8NoBOM
} catch {
    throw "GitHub accepted publication of $tag but public verification failed. Inspect the public release before retrying. $_"
}
Write-Host "Published verified release $tag."
