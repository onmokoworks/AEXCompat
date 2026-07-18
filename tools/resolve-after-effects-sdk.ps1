param([AllowEmptyString()][string]$SdkRoot)

$header = if ($SdkRoot) {
    Join-Path $SdkRoot "Examples\Headers\AE_Effect.h"
}
if (-not $SdkRoot -or -not (Test-Path -LiteralPath $header -PathType Leaf)) {
    throw "Set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root, then open a new shell."
}

(Resolve-Path -LiteralPath $SdkRoot).Path
