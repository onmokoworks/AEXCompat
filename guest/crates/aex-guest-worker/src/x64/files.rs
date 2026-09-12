// Explicit read-only guest assets. Guest path strings never become host paths.
const MAX_GUEST_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_GUEST_STREAM_BYTES: usize = 128 * 1024 * 1024;
const GUEST_STREAM_BASE: u64 = WORLD_DATA_END;
const MAX_GUEST_STREAM_OPENS: u64 = 4096;

#[derive(Default)]
struct GuestFiles {
    sources: BTreeMap<String, std::path::PathBuf>,
    streams: BTreeMap<u64, GuestFileStream>,
    next_stream: u64,
    live_bytes: usize,
    reports: Vec<TraceModule>,
}
struct GuestFileStream {
    bytes: Box<[u8]>,
    position: usize,
}

fn guest_file_name(name: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 1024 || !name.is_ascii() || name.contains('\0') {
        return Err("unsupported guest asset path encoding or length".into());
    }
    let normalized = name.replace('\\', "/").to_ascii_lowercase();
    if normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("ambiguous guest asset path".into());
    }
    Ok(normalized)
}

impl GuestFiles {
    fn from_environment() -> Result<Self, GuestError> {
        match std::env::var_os("AEXCOMPAT_GUEST_FILES") {
            Some(path) => Self::from_manifest(std::path::Path::new(&path)),
            None => Ok(Self::default()),
        }
    }

    fn from_manifest(path: &std::path::Path) -> Result<Self, GuestError> {
        use std::io::Read;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Manifest {
            files: Vec<Entry>,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Entry {
            name: String,
            path: std::path::PathBuf,
        }
        let read = || -> Result<Self, String> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 1024 * 1024 {
                return Err("asset manifest exceeds 1 MiB".into());
            }
            let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if manifest.files.len() > 8192 {
                return Err("asset manifest exceeds 8192 files".into());
            }
            let mut files = Self::default();
            for entry in manifest.files {
                let name = guest_file_name(&entry.name)?;
                let source = if entry.path.is_absolute() {
                    entry.path
                } else {
                    path.parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join(entry.path)
                };
                if files.sources.insert(name, source).is_some() {
                    return Err("duplicate guest asset path".into());
                }
            }
            Ok(files)
        };
        read().map_err(|e| GuestError::Callback(format!("guest asset manifest: {e}")))
    }
}

fn open_guest_stream(
    unicorn: &mut Unicorn<'_, GuestState>,
    filename: &[u8],
    mode: &[u8],
) -> Result<(u64, u32), String> {
    use std::io::Read;
    if filename.is_empty() || !valid_fopen_mode(mode) {
        return Ok((0, 22));
    }
    let filename =
        std::str::from_utf8(filename).map_err(|_| "unsupported fopen filename encoding")?;
    let name = guest_file_name(filename)?;
    let mode = trim_leading_crt_mode_spaces(mode);
    // The mounted namespace is read-only, including existing files opened r+.
    if mode[0] != b'r' || mode.contains(&b'+') || mode.contains(&b'D') {
        return Ok((0, 13));
    }

    let Some(source) = unicorn.get_data().guest_files.sources.get(&name).cloned() else {
        return Ok((0, 2));
    };
    if mode.contains(&b',') {
        return Err("Unicode fopen streams are not implemented".into());
    }
    match std::fs::metadata(&source) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok((0, 13)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 2)),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok((0, 13)),
        Err(e) => return Err(format!("guest asset metadata: {e}")),
    }
    let files = &unicorn.get_data().guest_files;
    if files.streams.len() >= 64 || files.next_stream >= MAX_GUEST_STREAM_OPENS {
        return Ok((0, 24));
    }
    let bound = MAX_GUEST_FILE_BYTES.min(MAX_GUEST_STREAM_BYTES - files.live_bytes);
    let file = match std::fs::File::open(&source) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 2)),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok((0, 13)),
        Err(e) => return Err(format!("open guest asset {name:?}: {e}")),
    };
    if !file
        .metadata()
        .map_err(|e| format!("asset metadata: {e}"))?
        .is_file()
    {
        return Ok((0, 13));
    }
    let mut bytes = Vec::new();
    file.take(bound as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read guest asset: {e}"))?;
    if bytes.len() > bound {
        return Err("guest stream byte capacity exceeded".into());
    }
    let sha = format!("{:x}", Sha256::digest(&bytes));
    if !mode.contains(&b'b') {
        // Windows text input: CRLF becomes LF; CTRL-Z ends the stream.
        let end = bytes.iter().position(|b| *b == 0x1a).unwrap_or(bytes.len());
        let mut output = 0;
        let mut input = 0;
        while input < end {
            if bytes[input] == b'\r' && input + 1 < end && bytes[input + 1] == b'\n' {
                input += 1;
            }
            bytes[output] = bytes[input];
            output += 1;
            input += 1;
        }
        bytes.truncate(output);
    }
    let token = GUEST_STREAM_BASE + unicorn.get_data().guest_files.next_stream * PAGE_SIZE;
    unicorn
        .mem_map(token, PAGE_SIZE, Prot::READ)
        .map_err(|e| format!("guest FILE token allocation: {e}"))?;
    let files = &mut unicorn.get_data_mut().guest_files;
    files.next_stream += 1;
    files.live_bytes += bytes.len();
    files.streams.insert(
        token,
        GuestFileStream {
            bytes: bytes.into_boxed_slice(),
            position: 0,
        },
    );
    files.reports.push(TraceModule {
        name,
        kind: "guest_asset",
        sha256: Some(sha),
        symbols: vec![],
    });
    Ok((token, 0))
}

fn emulate_fopen(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let name = read_win64_import_argument(unicorn, 0)?;
        let mode = read_win64_import_argument(unicorn, 1)?;
        if name == 0 || mode == 0 {
            unicorn.get_data_mut().crt_errno = 22;
            return Ok(0);
        }
        let name = read_crt_stdio_c_string(unicorn, name, MAX_CRT_STRING_BYTES, "fopen filename")?;
        let mode = read_crt_stdio_c_string(unicorn, mode, 64, "fopen mode")?;
        let (stream, errno) = open_guest_stream(unicorn, &name, &mode)?;
        if errno != 0 {
            unicorn.get_data_mut().crt_errno = errno;
        }
        Ok(stream)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_guest_stdio(unicorn: &mut Unicorn<'_, GuestState>, import: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        if import == LegacyWin64Import::Fgetc {
            let token = read_win64_import_argument(unicorn, 0)?;
            let stream = unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&token)
                .ok_or("getc received stale or foreign FILE")?;
            return Ok(match stream.bytes.get(stream.position) {
                Some(byte) => {
                    let value = u64::from(*byte);
                    stream.position += 1;
                    value
                }
                None => u64::from(u32::MAX), // EOF is an int, not a signed byte.
            });
        }
        if import == LegacyWin64Import::Fclose {
            let token = read_win64_import_argument(unicorn, 0)?;
            if !unicorn.get_data().guest_files.streams.contains_key(&token) {
                return Err("fclose received stale or foreign FILE".into());
            }
            unicorn
                .mem_unmap(token, PAGE_SIZE)
                .map_err(|e| format!("fclose token unmap: {e}"))?;
            let files = &mut unicorn.get_data_mut().guest_files;
            let stream = files.streams.remove(&token).unwrap();
            files.live_bytes -= stream.bytes.len();
            return Ok(0);
        }
        let output = read_win64_import_argument(unicorn, 0)?;
        let size = read_win64_import_argument(unicorn, 1)?;
        let count = read_win64_import_argument(unicorn, 2)?;
        let token = read_win64_import_argument(unicorn, 3)?;
        if size == 0 || count == 0 {
            return Ok(0);
        }
        let wanted = size.checked_mul(count).ok_or("fread size overflow")?;
        if wanted > MAX_GUEST_FILE_BYTES as u64 {
            return Err("fread request exceeds byte bound".into());
        }
        let stream = unicorn
            .get_data()
            .guest_files
            .streams
            .get(&token)
            .ok_or("fread received stale or foreign FILE")?;
        let actual = (wanted as usize).min(stream.bytes.len() - stream.position);
        if actual == 0 {
            return Ok(0);
        }
        if output == 0 || !guest_range_has_permission(unicorn, output, actual as u64, Prot::WRITE)?
        {
            return Err("fread output is not writable".into());
        }
        let bytes = stream.bytes[stream.position..stream.position + actual].to_vec();
        unicorn
            .mem_write(output, &bytes)
            .map_err(|e| format!("fread output write: {e}"))?;
        unicorn
            .get_data_mut()
            .guest_files
            .streams
            .get_mut(&token)
            .unwrap()
            .position += actual;
        Ok(actual as u64 / size)
    })();
    finish_guest_stdio(unicorn, result);
}

fn finish_guest_stdio(unicorn: &mut Unicorn<'_, GuestState>, result: Result<u64, String>) {
    match result {
        Ok(value) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, value);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}
