// Explicit read-only guest assets. Guest path strings never become host paths.
const MAX_GUEST_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_GUEST_STREAM_BYTES: usize = 128 * 1024 * 1024;
const GUEST_STREAM_BASE: u64 = WORLD_DATA_END;
const MAX_GUEST_STREAM_OPENS: u64 = 4096;
const GUEST_STREAM_BUFFER_BASE: u64 = GUEST_STREAM_BASE + MAX_GUEST_STREAM_OPENS * PAGE_SIZE;

#[derive(Default)]
struct GuestFiles {
    sources: BTreeMap<String, std::path::PathBuf>,
    streams: BTreeMap<u64, GuestFileStream>,
    next_stream: u64,
    standard_streams: [Option<u64>; 3],
    live_bytes: usize,
    reports: Vec<TraceModule>,
    searches: BTreeMap<u64, GuestFileSearch>,
    next_search: u64,
}
struct GuestFileStream {
    bytes: Box<[u8]>,
    position: usize,
    readable: bool,
    buffer_state: Option<u64>,
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
            readable: true,
            buffer_state: None,
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

// Standard FILE objects have stable identities, including after fclose. The
// worker has no guest stdin input; stdout/stderr are output-only. Output calls
// remain explicit unsupported imports until their capture semantics are provided.
fn emulate_acrt_iob_func(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let index = read_win64_import_argument(unicorn, 0)? as u32 as usize;
        if index >= 3 {
            return Err("__acrt_iob_func index is outside the three standard streams".into());
        }
        let files = &unicorn.get_data().guest_files;
        if let Some(token) = files.standard_streams[index] {
            return Ok(token);
        }
        if files.streams.len() >= 64 || files.next_stream >= MAX_GUEST_STREAM_OPENS {
            return Err("standard FILE token capacity exceeded".into());
        }
        let token = GUEST_STREAM_BASE + files.next_stream * PAGE_SIZE;
        unicorn
            .mem_map(token, PAGE_SIZE, Prot::READ)
            .map_err(|e| format!("standard FILE token allocation: {e}"))?;
        let files = &mut unicorn.get_data_mut().guest_files;
        files.next_stream += 1;
        files.standard_streams[index] = Some(token);
        files.streams.insert(
            token,
            GuestFileStream {
                bytes: Box::default(),
                position: 0,
                readable: index == 0,
                buffer_state: None,
            },
        );
        Ok(token)
    })();
    finish_guest_stdio(unicorn, result);
}

// _get_stream_buffer_pointers returns addresses of the FILE's char* base,
// char* cursor and int remaining-count cells, not the buffer values themselves.
// The current guest streams are unbuffered: all three values start at zero.
fn emulate_get_stream_buffer_pointers(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let token = read_win64_import_argument(unicorn, 0)?;
        let outputs = [
            read_win64_import_argument(unicorn, 1)?,
            read_win64_import_argument(unicorn, 2)?,
            read_win64_import_argument(unicorn, 3)?,
        ];
        let state = unicorn
            .get_data()
            .guest_files
            .streams
            .get(&token)
            .ok_or("stream buffer query received stale or foreign FILE")?
            .buffer_state;
        // Validate every output before allocating or writing anything.
        for output in outputs {
            if output != 0 && !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
                return Err("stream buffer pointer output is not writable".into());
            }
        }
        if outputs.iter().all(|output| *output == 0) {
            return Ok(0);
        }
        let state = match state {
            Some(state) => state,
            None => {
                let offset = token
                    .checked_sub(GUEST_STREAM_BASE)
                    .filter(|offset| {
                        *offset < MAX_GUEST_STREAM_OPENS * PAGE_SIZE && *offset % PAGE_SIZE == 0
                    })
                    .ok_or("stream buffer FILE token is outside its namespace")?;
                let state = GUEST_STREAM_BUFFER_BASE + offset;
                unicorn
                    .mem_map(state, PAGE_SIZE, Prot::READ | Prot::WRITE)
                    .map_err(|error| format!("stream buffer state allocation: {error}"))?;
                unicorn
                    .get_data_mut()
                    .guest_files
                    .streams
                    .get_mut(&token)
                    .unwrap()
                    .buffer_state = Some(state);
                state
            }
        };
        for (output, offset) in outputs.into_iter().zip([0u64, 8, 16]) {
            if output != 0 {
                unicorn
                    .mem_write(output, &(state + offset).to_le_bytes())
                    .map_err(|error| format!("stream buffer pointer output: {error}"))?;
            }
        }
        Ok(0)
    })();
    finish_guest_stdio(unicorn, result);
}

fn require_unbuffered_guest_stream(
    unicorn: &Unicorn<'_, GuestState>,
    token: u64,
) -> Result<(), String> {
    let stream = unicorn
        .get_data()
        .guest_files
        .streams
        .get(&token)
        .ok_or("stdio received stale or foreign FILE")?;
    if let Some(state) = stream.buffer_state {
        if !guest_range_has_permission(unicorn, state, 20, Prot::READ)? {
            return Err("FILE buffer state is not readable".into());
        }
        let mut values = [0u8; 20];
        unicorn
            .mem_read(state, &mut values)
            .map_err(|error| format!("FILE buffer state read: {error}"))?;
        if values != [0; 20] {
            return Err("guest-modified FILE buffering state is not implemented".into());
        }
    }
    Ok(())
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
            require_unbuffered_guest_stream(unicorn, token)?;
            let stream = unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&token)
                .ok_or("getc received stale or foreign FILE")?;
            if !stream.readable {
                return Err("getc received an output-only FILE".into());
            }
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
            if !unicorn
                .get_data()
                .guest_files
                .standard_streams
                .contains(&Some(token))
            {
                unicorn
                    .mem_unmap(token, PAGE_SIZE)
                    .map_err(|e| format!("fclose token unmap: {e}"))?;
            }
            if let Some(state) = unicorn.get_data().guest_files.streams[&token].buffer_state {
                unicorn
                    .mem_unmap(state, PAGE_SIZE)
                    .map_err(|error| format!("fclose buffer state unmap: {error}"))?;
            }
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
        require_unbuffered_guest_stream(unicorn, token)?;
        let stream = unicorn
            .get_data()
            .guest_files
            .streams
            .get(&token)
            .ok_or("fread received stale or foreign FILE")?;
        if !stream.readable {
            return Err("fread received an output-only FILE".into());
        }
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

struct GuestFileSearch {
    records: Vec<[u8; 320]>,
    position: usize,
}

fn guest_star_match(pattern: &[u8], name: &[u8]) -> bool {
    // A trailing DOS dot-star also matches an absent extension.
    if let Some(prefix) = pattern.strip_suffix(b".*") {
        if guest_star_match(prefix, name) {
            return true;
        }
    }
    let (mut p, mut n, mut star, mut retry) = (0, 0, None, 0);
    while n < name.len() {
        if p < pattern.len() && pattern[p] == name[n] {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = n;
        } else if let Some(s) = star {
            retry += 1;
            n = retry;
            p = s + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn guest_find_records(files: &GuestFiles, query: &str) -> Result<Vec<[u8; 320]>, String> {
    let query = guest_file_name(query)?;
    let (directory, pattern) = query
        .rsplit_once('/')
        .ok_or("relative file search is unsupported")?;
    if directory.contains(['*', '?']) || pattern.contains('?') || pattern.ends_with('.') {
        return Err("unsupported DOS search pattern".into());
    }
    let prefix = format!("{directory}/");
    let mut candidates: BTreeMap<String, Option<&std::path::PathBuf>> = BTreeMap::new();
    for (name, source) in &files.sources {
        if let Some(rest) = name.strip_prefix(&prefix) {
            let leaf = rest.split('/').next().unwrap();
            if !guest_star_match(pattern.as_bytes(), leaf.as_bytes()) {
                continue;
            }
            candidates
                .entry(leaf.to_string())
                .or_insert(if rest.contains('/') {
                    None
                } else {
                    Some(source)
                });
        }
    }
    let mut records = Vec::new();
    for (name, source) in candidates {
        if name.len() >= 260 {
            return Err("search filename exceeds WIN32_FIND_DATAA capacity".into());
        }
        let mut record = [0u8; 320];
        if let Some(source) = source {
            let metadata = match std::fs::metadata(source) {
                Ok(value) => value,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("search metadata: {e}")),
            };
            if !metadata.is_file() {
                return Err("mounted search asset is not a regular file".into());
            }
            record[..4].copy_from_slice(&1u32.to_le_bytes()); // read-only guest mount
            for (offset, time) in [
                (4, metadata.created()),
                (12, metadata.accessed()),
                (20, metadata.modified()),
            ] {
                if let Ok(time) = time {
                    record[offset..offset + 8]
                        .copy_from_slice(&windows_filetime(time)?.to_le_bytes());
                }
            }
            record[28..32].copy_from_slice(&((metadata.len() >> 32) as u32).to_le_bytes());
            record[32..36].copy_from_slice(&(metadata.len() as u32).to_le_bytes());
        } else {
            record[..4].copy_from_slice(&0x10u32.to_le_bytes()); // virtual containing directory
        }
        record[44..44 + name.len()].copy_from_slice(name.as_bytes());
        records.push(record);
    }
    Ok(records)
}

fn emulate_guest_file_search(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        let first = operation == LegacyWin64Import::FindFirstFileA;
        let token_or_name = read_win64_import_argument(unicorn, 0)?;
        if operation == LegacyWin64Import::FindClose {
            if unicorn
                .get_data_mut()
                .guest_files
                .searches
                .remove(&token_or_name)
                .is_some()
            {
                return Ok(1);
            }
            unicorn.get_data_mut().windows_last_error = 6;
            return Ok(0);
        }
        let output = read_win64_import_argument(unicorn, 1)?;
        let (token, record, new_records) = if first {
            let query = read_crt_stdio_c_string(unicorn, token_or_name, 1025, "file search")?;
            let query =
                std::str::from_utf8(&query).map_err(|_| "unsupported file search encoding")?;
            let files = &unicorn.get_data().guest_files;
            if files.searches.len() >= 64 || files.next_search >= 4096 {
                return Err("file search handle capacity exceeded".into());
            }
            let normalized = guest_file_name(query)?;
            let directory = normalized
                .rsplit_once('/')
                .ok_or("relative file search is unsupported")?
                .0;
            let prefix = format!("{directory}/");
            if !files.sources.keys().any(|name| name.starts_with(&prefix)) {
                unicorn.get_data_mut().windows_last_error = 3;
                return Ok(u64::MAX);
            }
            let records = guest_find_records(files, query)?;
            let Some(record) = records.first().copied() else {
                unicorn.get_data_mut().windows_last_error = 2;
                return Ok(u64::MAX);
            };
            if records.len()
                + files
                    .searches
                    .values()
                    .map(|s| s.records.len())
                    .sum::<usize>()
                > 32768
            {
                return Err("file search record capacity exceeded".into());
            }
            (0x900000000 + files.next_search * 16, record, Some(records))
        } else {
            let Some(search) = unicorn.get_data().guest_files.searches.get(&token_or_name) else {
                unicorn.get_data_mut().windows_last_error = 6;
                return Ok(0);
            };
            let Some(record) = search.records.get(search.position).copied() else {
                unicorn.get_data_mut().windows_last_error = 18;
                return Ok(0);
            };
            (token_or_name, record, None)
        };
        if output == 0 || !guest_range_has_permission(unicorn, output, 320, Prot::WRITE)? {
            return Err("file search output is not fully writable".into());
        }
        unicorn
            .mem_write(output, &record)
            .map_err(|e| format!("file search output: {e}"))?;
        let files = &mut unicorn.get_data_mut().guest_files;
        if let Some(records) = new_records {
            files.searches.insert(
                token,
                GuestFileSearch {
                    records,
                    position: 1,
                },
            );
            files.next_search += 1;
            Ok(token)
        } else {
            files.searches.get_mut(&token).unwrap().position += 1;
            Ok(1)
        }
    })();
    finish_guest_stdio(unicorn, result);
}
