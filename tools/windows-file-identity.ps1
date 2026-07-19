if (-not ('AexCompatLockedFileIdentity' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

public static class AexCompatLockedFileIdentity {
    [StructLayout(LayoutKind.Sequential)]
    private struct BY_HANDLE_FILE_INFORMATION {
        public uint FileAttributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME CreationTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastAccessTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWriteTime;
        public uint VolumeSerialNumber;
        public uint FileSizeHigh;
        public uint FileSizeLow;
        public uint NumberOfLinks;
        public uint FileIndexHigh;
        public uint FileIndexLow;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint GetFinalPathNameByHandleW(
        SafeFileHandle file, StringBuilder path, uint length, uint flags);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetFileInformationByHandle(
        SafeFileHandle file, out BY_HANDLE_FILE_INFORMATION information);

    public static string FinalPath(SafeFileHandle file) {
        var buffer = new StringBuilder(32768);
        uint written = GetFinalPathNameByHandleW(file, buffer, 32768, 0);
        if (written == 0 || written >= 32768) throw new Win32Exception(Marshal.GetLastWin32Error());
        return buffer.ToString();
    }

    public static string FileId(SafeFileHandle file) {
        BY_HANDLE_FILE_INFORMATION value;
        if (!GetFileInformationByHandle(file, out value))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return String.Format("{0:x8}:{1:x8}:{2:x8}",
            value.VolumeSerialNumber, value.FileIndexHigh, value.FileIndexLow);
    }
}
'@
}

function Get-Sha256HexFromText([string]$Value) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Value)
        ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally { $sha.Dispose() }
}

function ConvertFrom-FinalHandlePath([string]$Path) {
    if ($Path.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) {
        return '\\' + $Path.Substring(8)
    }
    if ($Path.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) {
        return $Path.Substring(4)
    }
    $Path
}

function Get-LockedFileIdentity([System.IO.FileStream]$Stream) {
    $streamPath = ConvertFrom-FinalHandlePath `
        ([AexCompatLockedFileIdentity]::FinalPath($Stream.SafeFileHandle))
    $streamPath = [System.IO.Path]::GetFullPath($streamPath)
    $Stream.Position = 0
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $contentHash = ([BitConverter]::ToString($sha.ComputeHash($Stream))).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose() }
    $Stream.Position = 0
    [ordered]@{
        sha256 = $contentHash
        final_path = $streamPath
        canonical_path_sha256 = Get-Sha256HexFromText $streamPath.ToLowerInvariant()
        file_id = [AexCompatLockedFileIdentity]::FileId($Stream.SafeFileHandle)
    }
}
