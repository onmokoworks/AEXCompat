param(
    [AllowEmptyString()][string]$Generator,
    [AllowEmptyString()][string]$Architecture
)

$ErrorActionPreference = 'Stop'

# build-*.ps1 が configure 時に追加で渡す引数を generator ごとに組み立てる (#1510)。
#
# - VS 系 generator だけが -A (platform) を受け付ける。Ninja 系に -A x64 を渡すと
#   configure が "does not support platform specification" で失敗する。
# - AEXCOMPAT_COMPILE_CACHE=sccache のとき CMAKE_<LANG>_COMPILER_LAUNCHER を差す。
#   launcher は Ninja / Makefile 系にしか効かないので、VS generator のときは
#   差さない (差しても無視されるわけではなく、効かないまま設定だけ残る)。
# - Ninja 系は cl / link / rc / mt を PATH から解決するので、vcvars 済みの
#   環境を要求する。CI は export-msvc-dev-env.ps1 が job 環境に用意する。

$result = @()

if ($Generator -like 'Visual Studio*') {
    if ($Architecture) {
        $result += @('-A', $Architecture)
    }
} else {
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        throw "generator '$Generator' requires the MSVC environment on PATH; run from a vcvars64 shell or export it with tools/export-msvc-dev-env.ps1"
    }
    if ($env:AEXCOMPAT_COMPILE_CACHE -eq 'sccache') {
        $result += @('-DCMAKE_C_COMPILER_LAUNCHER=sccache', '-DCMAKE_CXX_COMPILER_LAUNCHER=sccache')
    }
}

$result
