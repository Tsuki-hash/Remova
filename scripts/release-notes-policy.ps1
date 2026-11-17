function Test-UnsignedInstallers([string]$BundleDir, [string]$Version) {
    foreach ($name in @("Remova_${Version}_x64-setup.exe", "Remova_${Version}_x64_zh-CN.msi")) {
        $signature = Get-AuthenticodeSignature -LiteralPath (Join-Path $BundleDir $name)
        if ($signature.Status -eq 'NotSigned') { return $true }
        if (!$signature.SignerCertificate) { throw "Could not determine the signing mode of $name." }
    }
    return $false
}

function Assert-ReleaseDownloadNotes([string]$Body, [string]$Version, [switch]$Unsigned) {
    $lines = $Body -split '\r?\n'
    $expected = @(
        '### 下载与校验'
        ''
        '从此发布页下载所选发行包和 `SHA256SUMS.txt`。在下载目录打开 PowerShell，将示例文件名替换为实际下载的 EXE、MSI 或 ZIP 文件名。'
        ''
        '```powershell'
        "Get-FileHash -LiteralPath '.\Remova_${Version}_x64-setup.exe' -Algorithm SHA256"
        '```'
        ''
        '将输出的 Hash 与 `SHA256SUMS.txt` 中同名文件的摘要比对。一致后再运行安装程序或解压便携版。'
    )
    if ($Unsigned) {
        $expected = @('本版提供未签名安装包，更新请手动下载。Windows 可能显示发布者或 SmartScreen 提示。', '') + $expected
    }

    # The instructions must be visible at the start of the release, after its
    # optional Markdown heading. Tokens in comments or later examples do not count.
    $index = 0
    if ($lines[0] -cne $expected[0] -and $lines[0] -match '^#{1,6} [^<]+$') { $index++ }
    while ($index -lt $lines.Count -and !$lines[$index].Trim()) { $index++ }
    foreach ($line in $expected) {
        if ($index -ge $lines.Count -or $lines[$index].TrimEnd() -cne $line) {
            throw 'Release notes must start with the download verification instructions and the applicable unsigned notice.'
        }
        $index++
    }
}
