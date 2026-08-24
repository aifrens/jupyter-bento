# 出厂环境快照构建脚本（Windows / PowerShell）
# 用法: .\build-snapshot.ps1 [-Target win-x64] [-OutDir <目录>]
# 产物: env-factory.tar.zst （python-build-standalone 3.9.7 + 完整哈希锁定依赖预装）
param(
  [ValidateSet("win-x64")] [string]$Target = "win-x64",
  [string]$OutDir = "$PSScriptRoot\..\app\src-tauri\resources",
  [string]$BootstrapPython = $(if ($env:BOOTSTRAP_PYTHON) { $env:BOOTSTRAP_PYTHON } else { "python" })
)
$ErrorActionPreference = "Stop"

# 强制 UTF-8：Windows 运行器默认按系统区域编码（cp1252/cp936）解码文本，
# 会让 pip 解析 requirements 等文件时 UnicodeDecodeError；构建期统一 UTF-8。
$env:PYTHONUTF8 = "1"

$Mirror = $(if ($env:JUPITER_PYPI_MIRROR) { $env:JUPITER_PYPI_MIRROR } else { "https://mirrors.aliyun.com/pypi/simple/" })
# 网络兜底：防跨国下载大 wheel 时 ReadTimeout
$env:PIP_DEFAULT_TIMEOUT = "120"
$env:PIP_RETRIES = "10"
$BootstrapVersion = "3.13.5"
$PbsTag = "20211017"
$PbsTs  = "20211017T1616"
$Asset  = "cpython-3.9.7-x86_64-pc-windows-msvc-shared-pgo-$PbsTs.tar.zst"
$Url    = "https://github.com/astral-sh/python-build-standalone/releases/download/$PbsTag/$Asset"
$Req    = Join-Path $PSScriptRoot "requirements-win-x64.lock.txt"
$BootstrapLock = Join-Path $PSScriptRoot "requirements-bootstrap-win-x64.lock.txt"
$Checksums = Join-Path $PSScriptRoot "python-build-standalone-20211017.sha256"

# Windows PowerShell 5.1 和部分 PowerShell 7 配置不会因为原生命令返回非零
# 而响应 $ErrorActionPreference。所有处理可执行构建输入的 python/pip 调用都
# 必须显式检查退出码，避免哈希校验、安装或压缩失败后继续生成不完整快照。
function Assert-NativeCommandSucceeded {
  param(
    [Parameter(Mandatory = $true)][string]$Operation,
    [Parameter(Mandatory = $true)][int]$ExitCode
  )

  if ($ExitCode -ne 0) {
    throw "$Operation 失败（退出码 $ExitCode）"
  }
}

# 供应链完整性边界：下载的 Python 会在后续构建中被直接执行，必须在解压前
# 与仓库内固定的 SHA-256 匹配。摘要缺失或不匹配均终止构建。
function Get-PinnedSha256 {
  param([Parameter(Mandatory = $true)][string]$AssetName)

  if (-not (Test-Path -LiteralPath $Checksums -PathType Leaf)) {
    throw "缺少 Python 归档校验清单: $Checksums"
  }

  $Entries = @(
    @(Get-Content -LiteralPath $Checksums) | ForEach-Object {
      $Fields = $_.Trim() -split '\s+', 2
      if ($Fields.Count -eq 2 -and $Fields[1] -ceq $AssetName) {
        if ($Fields[0] -notmatch '^[0-9a-fA-F]{64}$') {
          throw "校验清单中包含无效 SHA-256: $AssetName"
        }
        $Fields[0].ToLowerInvariant()
      }
    }
  )
  if (@($Entries).Count -ne 1) {
    throw "校验清单中必须有且仅有一个有效 SHA-256: $AssetName"
  }
  return @($Entries)[0]
}

function Assert-ArchiveSha256 {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$AssetName
  )

  $Expected = Get-PinnedSha256 -AssetName $AssetName
  $Actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($Actual -cne $Expected) {
    throw "Python 归档 SHA-256 校验失败: $AssetName`n  期望: $Expected`n  实际: $Actual"
  }
  Write-Host "==> SHA-256 校验通过: $AssetName"
}

$Work = Join-Path $env:TEMP ("snap-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Work | Out-Null

try {
  # 构建 Python 会解压并重新压缩可执行运行时；本地与 CI 都必须精确为
  # 3.13.5，并只安装仓库哈希锁允许的 bootstrap 工具。
  $ActualBootstrapVersionOutput = & $BootstrapPython -c "import platform; print(platform.python_version())"
  Assert-NativeCommandSucceeded -Operation "检查构建 Python 版本" -ExitCode $LASTEXITCODE
  $ActualBootstrapVersion = ($ActualBootstrapVersionOutput -join "`n").Trim()
  if ($ActualBootstrapVersion -cne $BootstrapVersion) {
    throw "构建 Python 版本不匹配：需要 $BootstrapVersion，实际 $ActualBootstrapVersion ($BootstrapPython)"
  }
  if (-not (Test-Path -LiteralPath $BootstrapLock -PathType Leaf)) {
    throw "缺少构建工具哈希锁: $BootstrapLock"
  }
  if (-not (Test-Path -LiteralPath $Req -PathType Leaf)) {
    throw "缺少运行时依赖哈希锁: $Req"
  }

  Write-Host "==> [1/4] 下载独立 Python 3.9.7 ($Target)"
  Invoke-WebRequest -Uri $Url -OutFile "$Work\pbs.tar.zst"
  Assert-ArchiveSha256 -Path "$Work\pbs.tar.zst" -AssetName $Asset

  Write-Host "==> [2/4] 解压"
  & $BootstrapPython -m venv "$Work\venv"
  Assert-NativeCommandSucceeded -Operation "创建 bootstrap 虚拟环境" -ExitCode $LASTEXITCODE
  $BootstrapPy = "$Work\venv\Scripts\python.exe"
  & $BootstrapPy -m pip install -q --disable-pip-version-check `
    --require-hashes --only-binary=:all: --no-deps --no-cache-dir `
    --index-url $Mirror -r $BootstrapLock
  Assert-NativeCommandSucceeded -Operation "安装 bootstrap 哈希锁" -ExitCode $LASTEXITCODE
  & $BootstrapPy "$PSScriptRoot\tarzst.py" extract "$Work\pbs.tar.zst" "$Work\root"
  Assert-NativeCommandSucceeded -Operation "解压 Python 归档" -ExitCode $LASTEXITCODE

  $Py = "$Work\root\python\install\python.exe"
  Write-Host "==> [3/4] 安装锁定依赖（仅使用官方 wheel）"
  $Wheelhouse = "$Work\wheels"
  New-Item -ItemType Directory -Path $Wheelhouse | Out-Null
  & $Py -m pip download --disable-pip-version-check `
    --require-hashes --only-binary=:all: --no-deps `
    --platform win_amd64 --implementation cp --abi cp39 --python-version 3.9 `
    --dest $Wheelhouse --no-cache-dir --index-url $Mirror -r $Req
  Assert-NativeCommandSucceeded -Operation "下载运行时哈希锁 wheel" -ExitCode $LASTEXITCODE
  & $Py -m pip install --disable-pip-version-check `
    --require-hashes --only-binary=:all: --no-deps `
    --no-index --find-links $Wheelhouse --no-cache-dir -r $Req
  Assert-NativeCommandSucceeded -Operation "离线安装运行时哈希锁 wheel" -ExitCode $LASTEXITCODE
  & $Py -c "import numpy, pandas, scipy, matplotlib, sklearn, notebook; print('导入校验通过: numpy', numpy.__version__, '| pandas', pandas.__version__)"
  Assert-NativeCommandSucceeded -Operation "导入运行时依赖" -ExitCode $LASTEXITCODE

  Write-Host "==> [3.6/4] 性能优化：预编译字节码 + 预建 matplotlib 字体缓存"
  $null = & $Py -m compileall -q "$Work\root\python\install\Lib\site-packages"
  Assert-NativeCommandSucceeded -Operation "预编译运行时字节码" -ExitCode $LASTEXITCODE
  $env:MPLCONFIGDIR = "$Work\root\python\install\mpl-config"
  & $Py -c "import matplotlib.pyplot as plt; print('matplotlib 字体缓存已预建')"
  Assert-NativeCommandSucceeded -Operation "预建 matplotlib 字体缓存" -ExitCode $LASTEXITCODE
  Remove-Item Env:MPLCONFIGDIR -ErrorAction SilentlyContinue

  Write-Host "==> [3.7/4] 内置运行时兼容补丁 sitecustomize.py（修复 Windows realpath 尾随点崩溃）"
  Copy-Item "$PSScriptRoot\sitecustomize.py" "$Work\root\python\install\Lib\site-packages\sitecustomize.py"

  Write-Host "==> [3.5/4] 生成出厂包清单 factory-manifest.json（应用以此区分 内置/用户安装）"
  # 与 build-snapshot.sh 对等的必需步骤：该清单是应用端「内置 vs 用户安装」的
  # 唯一权威判定依据。缺失时应用静默回退到 16 个核心包清单，其余出厂包全部被
  # 误判为用户安装（Windows 历史事故），不得省略。
  $ManifestJson = & $Py -m pip list --format=json --disable-pip-version-check
  Assert-NativeCommandSucceeded -Operation "生成出厂包清单" -ExitCode $LASTEXITCODE
  # 用 .NET 写盘而非 PowerShell 重定向：强制 UTF-8 无 BOM，
  # 避免编码/BOM 差异导致应用端 JSON 解析失败
  [System.IO.File]::WriteAllText(
    "$Work\root\python\install\factory-manifest.json",
    ($ManifestJson -join "`n") + "`n",
    (New-Object System.Text.UTF8Encoding($false)))
  & $Py -c "import json,sys; d=json.load(open(sys.argv[1],encoding='utf-8')); assert isinstance(d,list) and len(d)>10, 'manifest too small'; print('factory manifest entries:', len(d))" "$Work\root\python\install\factory-manifest.json"
  Assert-NativeCommandSucceeded -Operation "校验出厂包清单" -ExitCode $LASTEXITCODE

  Write-Host "==> [4/4] 压缩出厂快照"
  New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  Move-Item "$Work\root\python\install" "$Work\python"
  & $BootstrapPy "$PSScriptRoot\tarzst.py" compress "$Work\python" "$OutDir\env-factory.tar.zst"
  Assert-NativeCommandSucceeded -Operation "压缩出厂快照" -ExitCode $LASTEXITCODE
  Write-Host "==> 完成: $OutDir\env-factory.tar.zst"
}
finally {
  Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
