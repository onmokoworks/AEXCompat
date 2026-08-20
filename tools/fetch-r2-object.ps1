<#
.SYNOPSIS
Downloads one object from a private Cloudflare R2 bucket over the S3 API (issue #1445).

.DESCRIPTION
CI needs the After Effects SDK zip from a bucket that is deliberately not public:
publishing the Adobe SDK at a URL anyone could fetch would be redistribution.
The bucket therefore stays private and the request carries an AWS SigV4
signature made from a read-only R2 API token.

The signing is implemented here rather than shelled out to a CLI because the two
runners this workflow uses do not share one. `windows-latest` ships the AWS CLI
but no rclone; the self-hosted `windows-real` runner is a workstation with
rclone but no AWS CLI. Installing either per run would add a network dependency
to the step whose whole job is to fetch one 4.6 MiB file.

Only GetObject is signed: the request has no body, so the payload hash is always
the SHA256 of the empty string and no streaming signature is involved.

The download lands in a sibling `.partial` file and is renamed onto -OutFile only
after -ExpectedSha256 matches, so a truncated transfer or an error page can never
be left behind looking like the SDK.

.PARAMETER Bucket
R2 bucket name. Addressed path-style, which is what the R2 S3 endpoint expects.

.PARAMETER Key
Object key inside the bucket, `/`-separated.

.PARAMETER OutFile
Destination path. Its directory must already exist.

.PARAMETER ExpectedSha256
Hex SHA256 the downloaded bytes must have. Optional, but omitting it means
nothing checks what arrived.

.PARAMETER Endpoint
R2 S3 endpoint (`https://<account>.r2.cloudflarestorage.com`). Defaults to
$env:R2_S3_ENDPOINT.

.PARAMETER AccessKeyId
R2 API token access key id. Defaults to $env:R2_ACCESS_KEY_ID.

.PARAMETER SecretAccessKey
R2 API token secret. Defaults to $env:R2_SECRET_ACCESS_KEY.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Bucket,
    [Parameter(Mandatory = $true)][string]$Key,
    [Parameter(Mandatory = $true)][string]$OutFile,
    [string]$ExpectedSha256 = '',
    [string]$Endpoint = $env:R2_S3_ENDPOINT,
    [string]$AccessKeyId = $env:R2_ACCESS_KEY_ID,
    [string]$SecretAccessKey = $env:R2_SECRET_ACCESS_KEY
)

$ErrorActionPreference = 'Stop'

function Assert-Provided {
    param([string]$Value, [string]$Name)
    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "$Name is not set. Provide the parameter or the matching environment variable."
    }
}

Assert-Provided $Endpoint 'Endpoint (R2_S3_ENDPOINT)'
Assert-Provided $AccessKeyId 'AccessKeyId (R2_ACCESS_KEY_ID)'
Assert-Provided $SecretAccessKey 'SecretAccessKey (R2_SECRET_ACCESS_KEY)'

# A mistyped or substituted endpoint would send the signed credential somewhere
# else, so the host is pinned to R2 instead of trusted from the secret.
$endpointUri = [Uri]$Endpoint.Trim().TrimEnd('/')
if ($endpointUri.Scheme -ne 'https') {
    throw "R2 endpoint must be https, got '$($endpointUri.Scheme)'"
}
if ($endpointUri.Host -notmatch '(?i)\.r2\.cloudflarestorage\.com$') {
    throw "R2 endpoint host is not an R2 S3 endpoint: $($endpointUri.Host)"
}
if ($endpointUri.AbsolutePath -notin @('', '/') -or $endpointUri.Query) {
    throw "R2 endpoint must be the bare origin, got '$Endpoint'"
}

function Get-Sha256Hex {
    param([byte[]]$Bytes)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return (($sha.ComputeHash($Bytes) | ForEach-Object { $_.ToString('x2') }) -join '')
    } finally {
        $sha.Dispose()
    }
}

function Get-HmacSha256 {
    param([byte[]]$KeyBytes, [string]$Message)
    $hmac = [System.Security.Cryptography.HMACSHA256]::new($KeyBytes)
    try {
        return $hmac.ComputeHash([Text.Encoding]::UTF8.GetBytes($Message))
    } finally {
        $hmac.Dispose()
    }
}

# SigV4 wants RFC 3986 encoding with only ALPHA / DIGIT / -._~ left literal.
# [Uri]::EscapeDataString disagrees with itself across .NET Framework and .NET
# (Framework leaves !*'() literal, .NET escapes them), which would silently
# produce a signature that does not match the request line for such a key.
function ConvertTo-SigV4Segment {
    param([string]$Value)
    $builder = [Text.StringBuilder]::new()
    foreach ($byte in [Text.Encoding]::UTF8.GetBytes($Value)) {
        $unreserved = ($byte -ge 0x41 -and $byte -le 0x5A) -or
                      ($byte -ge 0x61 -and $byte -le 0x7A) -or
                      ($byte -ge 0x30 -and $byte -le 0x39) -or
                      ($byte -in 0x2D, 0x2E, 0x5F, 0x7E)
        if ($unreserved) {
            [void]$builder.Append([char]$byte)
        } else {
            [void]$builder.AppendFormat('%{0:X2}', $byte)
        }
    }
    return $builder.ToString()
}

$normalizedKey = $Key.Trim('/')
if (-not $normalizedKey) {
    throw 'Key must name an object'
}
$segments = @($Bucket) + $normalizedKey.Split('/')
if ($segments | Where-Object { $_ -eq '' -or $_ -eq '.' -or $_ -eq '..' }) {
    throw "Key must not contain empty or relative path segments: $Key"
}
$canonicalUri = '/' + (($segments | ForEach-Object { ConvertTo-SigV4Segment $_ }) -join '/')

$region = 'auto'
$service = 's3'
$now = [DateTime]::UtcNow
$amzDate = $now.ToString('yyyyMMddTHHmmssZ')
$dateStamp = $now.ToString('yyyyMMdd')
$payloadHash = Get-Sha256Hex ([byte[]]@())

$canonicalHeaders = "host:$($endpointUri.Host)`nx-amz-content-sha256:$payloadHash`nx-amz-date:$amzDate`n"
$signedHeaders = 'host;x-amz-content-sha256;x-amz-date'
$canonicalRequest = "GET`n$canonicalUri`n`n$canonicalHeaders`n$signedHeaders`n$payloadHash"

$credentialScope = "$dateStamp/$region/$service/aws4_request"
$stringToSign = "AWS4-HMAC-SHA256`n$amzDate`n$credentialScope`n$(Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($canonicalRequest)))"

$signingKey = Get-HmacSha256 ([Text.Encoding]::UTF8.GetBytes("AWS4$SecretAccessKey")) $dateStamp
$signingKey = Get-HmacSha256 $signingKey $region
$signingKey = Get-HmacSha256 $signingKey $service
$signingKey = Get-HmacSha256 $signingKey 'aws4_request'
$signature = ((Get-HmacSha256 $signingKey $stringToSign) | ForEach-Object { $_.ToString('x2') }) -join ''

$authorization = "AWS4-HMAC-SHA256 Credential=$AccessKeyId/$credentialScope, SignedHeaders=$signedHeaders, Signature=$signature"
$requestUri = "https://$($endpointUri.Host)$canonicalUri"
$partial = "$OutFile.partial"
if (Test-Path -LiteralPath $partial) {
    Remove-Item -LiteralPath $partial -Force
}

Write-Host "fetching r2://$Bucket/$normalizedKey from $($endpointUri.Host)"

# HttpClient rather than Invoke-WebRequest: IWR routes -Headers Authorization
# through the typed header parser, which rejects the SigV4 value outright
# ("The format of value 'AWS4-HMAC-SHA256 Credential=...' is invalid").
# TryAddWithoutValidation sends it verbatim. The alternative - a presigned URL
# carrying the signature in the query string - was avoided because HTTP clients
# put the request URI in their error text, which would spill the access key id
# into CI logs.
Add-Type -AssemblyName System.Net.Http
if ($PSVersionTable.PSEdition -eq 'Desktop') {
    # On .NET Framework the handler still negotiates through
    # ServicePointManager, which on an untouched host can exclude TLS 1.2.
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}

$handler = [System.Net.Http.HttpClientHandler]::new()
# A signed request must not chase a redirect: the credential is bound to this
# host and path, so anything but a direct answer is a failure to report.
$handler.AllowAutoRedirect = $false
$client = [System.Net.Http.HttpClient]::new($handler)
$client.Timeout = [TimeSpan]::FromMinutes(5)
$request = $null
$response = $null
$responseStream = $null
$fileStream = $null
try {
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $requestUri)
    [void]$request.Headers.TryAddWithoutValidation('Authorization', $authorization)
    [void]$request.Headers.TryAddWithoutValidation('x-amz-content-sha256', $payloadHash)
    [void]$request.Headers.TryAddWithoutValidation('x-amz-date', $amzDate)
    $response = $client.SendAsync($request, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
    if (-not $response.IsSuccessStatusCode) {
        # R2's error body is XML naming the S3 error code (NoSuchKey,
        # SignatureDoesNotMatch, AccessDenied). It carries no credential.
        $body = ''
        try {
            $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        } catch {
            $body = '<error body unavailable>'
        }
        if ($body.Length -gt 500) {
            $body = $body.Substring(0, 500) + '...'
        }
        throw "R2 GetObject failed with HTTP $([int]$response.StatusCode) $($response.StatusCode) for r2://$Bucket/$normalizedKey : $body"
    }
    $responseStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
    $fileStream = [IO.File]::Create($partial)
    $responseStream.CopyTo($fileStream)
} catch {
    if ($null -ne $fileStream) {
        $fileStream.Dispose()
        $fileStream = $null
    }
    if (Test-Path -LiteralPath $partial) {
        Remove-Item -LiteralPath $partial -Force
    }
    throw
} finally {
    if ($null -ne $fileStream) { $fileStream.Dispose() }
    if ($null -ne $responseStream) { $responseStream.Dispose() }
    if ($null -ne $response) { $response.Dispose() }
    if ($null -ne $request) { $request.Dispose() }
    $client.Dispose()
    $handler.Dispose()
}

if (-not (Test-Path -LiteralPath $partial -PathType Leaf)) {
    throw "R2 GetObject reported success but wrote no file for r2://$Bucket/$normalizedKey"
}

$actualSha256 = (Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash
$size = (Get-Item -LiteralPath $partial).Length
if ($ExpectedSha256 -and $actualSha256 -ne $ExpectedSha256.Trim().ToUpperInvariant()) {
    Remove-Item -LiteralPath $partial -Force
    throw "R2 object hash mismatch for r2://$Bucket/$normalizedKey : expected $ExpectedSha256, got $actualSha256"
}

Move-Item -LiteralPath $partial -Destination $OutFile -Force
Write-Host "fetched r2://$Bucket/$normalizedKey size=$size sha256=$actualSha256"
