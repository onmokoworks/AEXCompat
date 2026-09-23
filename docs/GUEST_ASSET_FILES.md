# Guest asset files on the Unicorn backend

Set `AEXCOMPAT_GUEST_FILES` to a JSON manifest to make asset files available
through the emulated UCRT `fopen` and `fopen_s` imports. The manifest is loaded
before DLL and AEX initialization. For example:

```json
{"files":[{"name":"c:/Assets/config.txt","path":"assets/config.txt"}]}
```

`name` is the guest filename. Matching is ASCII case-insensitive and treats
backslashes as slashes. Paths containing empty, `.` or `..` components are
rejected. Non-ASCII paths are currently unsupported. `path` is the host source,
relative to the manifest directory or absolute. Only explicit entries are
resolved: guest filenames are never directly opened on the host. Duplicate
normalized names are an error. An unset manifest means an empty asset namespace.

The namespace is read-only. Write, append, update and delete-on-close modes
return `EACCES`; unmounted or missing source files return `ENOENT`. Binary reads
preserve bytes. Text reads convert CRLF to LF and stop at CTRL-Z. Unicode stream
modes are not implemented. `fread` advances an owned stream, returns the number
of complete elements, and copies partial final elements as well. `fclose`
releases the stream. Closed tokens are not reused within an engine; stale or
foreign stream operations stop with a diagnostic.

Limits are a 1 MiB manifest, 8192 entries, 64 MiB per file, 128 MiB of live stream
contents, 64 simultaneous streams, and 4096 successful opens per engine.
Nonregular source files are rejected. File contents are snapshotted when opened.
The SHA-256 of the original bytes, before text conversion, is recorded as a
`guest_asset` entry in execution trace modules. No expected digest gates opening.

Other CRT stream operations and Win32 file APIs do not yet share this namespace.
A complete plug-in lifecycle may require further imports; an opened asset alone
is not evidence of successful rendering or cleanup.
