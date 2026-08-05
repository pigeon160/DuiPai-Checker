# 一键发版脚本：版本号更新 → 构建安装包 → commit/tag → push → GitHub Release + 上传资产
#
# 用法（PowerShell）：
#   .\scripts\release.ps1 1.0.2
#   .\scripts\release.ps1 1.0.2 -Notes "修复 XXX；新增 YYY"
#   .\scripts\release.ps1 1.0.2 -DryRun   # 只更新版本号 + 构建，不推送不上传
#
# 依赖：git（schannel 凭据）、npm、cargo、curl

param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Notes = "",
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "版本号格式应为 x.y.z，实际：$Version"
}

function Set-Utf8([string]$path, [string]$content) {
    [System.IO.File]::WriteAllText($path, $content, (New-Object System.Text.UTF8Encoding($false)))
}

function Read-Utf8([string]$path) {
    [System.IO.File]::ReadAllText($path, (New-Object System.Text.UTF8Encoding($false)))
}

function Push-Retry([string]$desc, [scriptblock]$cmd) {
    for ($i = 1; $i -le 8; $i++) {
        Write-Host "  $desc（第 $i 次尝试）"
        & $cmd
        if ($LASTEXITCODE -eq 0) { return }
        Start-Sleep 10
    }
    throw "$desc 连续失败"
}

# ---- 1) 版本号 ----
$conf = Read-Utf8 "src-tauri\tauri.conf.json"
if ($conf -notmatch '"version":\s*"([^"]+)"') { throw "无法读取 tauri.conf.json 当前版本" }
$old = $Matches[1]
if ($old -eq $Version) { throw "当前版本已是 $Version" }
Write-Host "== 版本号 $old -> $Version =="
Set-Utf8 "src-tauri\tauri.conf.json" ($conf.Replace('"version": "' + $old + '"', '"version": "' + $Version + '"'))
foreach ($p in @("src-tauri\Cargo.toml", "crates\duipai-core\Cargo.toml")) {
    $t = Read-Utf8 $p
    Set-Utf8 $p ($t.Replace('version = "' + $old + '"', 'version = "' + $Version + '"'))
}

# ---- 2) 构建 ----
Write-Host "== 构建安装包（tauri build）=="
Get-Process duipai-checker -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
npm run tauri build
if ($LASTEXITCODE -ne 0) { throw "构建失败" }
$setup = Get-ChildItem "target\release\bundle\nsis\*-setup.exe" |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $setup) { throw "未找到安装包" }
Write-Host "  安装包：$($setup.FullName)"

if ($DryRun) {
    Write-Host ""
    Write-Host "== DryRun：跳过 commit/tag/push/release =="
    Write-Host "当前改动未提交（版本号 + 构建产物）"
    exit 0
}

# ---- 3) 提交 + tag ----
$msg = if ($Notes) { "v$Version`n`n$Notes" } else { "v$Version" }
Set-Utf8 "$env:TEMP\dp-rel-msg.txt" $msg
Write-Host "== 提交 + tag =="
git add -A
git commit -F "$env:TEMP\dp-rel-msg.txt"
Remove-Item "$env:TEMP\dp-rel-msg.txt" -ErrorAction SilentlyContinue
git tag "v$Version"

# ---- 4) push ----
Write-Host "== push main + tag =="
Push-Retry "push main" { git push origin HEAD 2>&1 | Out-Null }
Push-Retry "push tag v$Version" { git push origin "v$Version" 2>&1 | Out-Null }

# ---- 5) GitHub Release ----
$creds = "protocol=https`nhost=github.com`n`n" | git credential fill 2>$null
$token = ($creds | Select-String "^password=(.*)$").Matches[0].Groups[1].Value
if (-not $token) { throw "无法从 git 凭据获取 GitHub token" }

$body = @{ tag_name = "v$Version"; name = "v$Version"; body = $Notes } |
    ConvertTo-Json -Compress
Set-Utf8 "$env:TEMP\dp-rel-body.json" $body

Write-Host "== 创建 GitHub Release =="
$relJson = curl.exe -s -X POST -H "Authorization: token $token" -H "Accept: application/vnd.github+json" `
    --data-binary "@$env:TEMP\dp-rel-body.json" `
    "https://api.github.com/repos/pigeon160/DuiPai-Checker/releases"
Remove-Item "$env:TEMP\dp-rel-body.json" -ErrorAction SilentlyContinue
$rel = $relJson | ConvertFrom-Json
if (-not $rel.id) { throw "创建 Release 失败：$relJson" }
Write-Host "  Release：$($rel.html_url)"

# ---- 6) 上传资产 ----
$assetName = "duipai-checker_${Version}_x64-setup.exe"
Write-Host "== 上传安装包（$assetName）=="
$assetJson = curl.exe -s -X POST -H "Authorization: token $token" `
    -H "Accept: application/vnd.github+json" -H "Content-Type: application/octet-stream" `
    --data-binary "@$($setup.FullName)" `
    "https://uploads.github.com/repos/pigeon160/DuiPai-Checker/releases/$($rel.id)/assets?name=$assetName"
$asset = $assetJson | ConvertFrom-Json
if (-not $asset.id) { throw "上传资产失败：$assetJson" }

Write-Host ""
Write-Host "===== 发布完成 ====="
Write-Host "版本：v$Version"
Write-Host "Release：$($rel.html_url)"
Write-Host "安装包：$($asset.browser_download_url)"
