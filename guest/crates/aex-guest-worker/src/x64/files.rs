// Explicit read-only guest assets. Guest path strings never become host paths.
const MAX_GUEST_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_GUEST_STREAM_BYTES: usize = 128 * 1024 * 1024;
const GUEST_STREAM_BASE: u64 = WORLD_DATA_END;
const MAX_GUEST_STREAM_OPENS: u64 = 4096;
const GUEST_STREAM_BUFFER_BASE: u64 = GUEST_STREAM_BASE + MAX_GUEST_STREAM_OPENS * PAGE_SIZE;

#[derive(Default)]
struct GuestFiles {
    sources: BTreeMap<String, std::path::PathBuf>,
    directories: BTreeSet<String>,
    directory_dacls: BTreeMap<String, Vec<u8>>,
    directory_times: BTreeMap<String, [std::time::SystemTime; 3]>,
    directory_attributes: BTreeMap<String, u32>,
    streams: BTreeMap<u64, GuestFileStream>,
    descriptors: BTreeMap<i32, GuestFileStream>,
    descriptor_modes: BTreeMap<i32, i32>,
    next_descriptor: i32,
    next_stream: u64,
    standard_streams: [Option<u64>; 3],
    live_bytes: usize,
    reports: Vec<TraceModule>,
    searches: BTreeMap<u64, GuestFileSearch>,
    next_search: u64,
    windows_files: BTreeMap<u64, WindowsAssetFile>,
    next_windows_file: u64,
}

#[derive(Default)]
struct GuestConsoleRepeat {
    last_write: Vec<u8>,
    suppressed: u64,
    announced: u64,
}

impl GuestConsoleRepeat {
    fn write(&mut self, bytes: &[u8], output: &mut impl std::io::Write) -> std::io::Result<()> {
        if self.last_write == bytes {
            self.suppressed = self.suppressed.saturating_add(1);
            if self.suppressed.is_power_of_two() {
                self.write_summary(output)?;
            }
            return Ok(());
        }

        self.flush(output)?;
        output.write_all(bytes)?;
        self.last_write.clear();
        self.last_write.extend_from_slice(bytes);
        self.suppressed = 0;
        self.announced = 0;
        Ok(())
    }

    fn flush(&mut self, output: &mut impl std::io::Write) -> std::io::Result<()> {
        if self.suppressed != self.announced {
            self.write_summary(output)?;
        }
        Ok(())
    }

    fn write_summary(&mut self, output: &mut impl std::io::Write) -> std::io::Result<()> {
        writeln!(
            output,
            "aex_guest_stdio: previous message cumulative repeat count is {}",
            self.suppressed
        )?;
        self.announced = self.suppressed;
        Ok(())
    }
}

struct GuestFileStream {
    name: Option<String>,
    bytes: Vec<u8>,
    position: usize,
    readable: bool,
    share_read_access: bool,
    eof: bool,
    buffer_state: Option<u64>,
    fast_buffer: Option<u64>,
    console_repeat: GuestConsoleRepeat,
}

#[track_caller]
fn guest_file_name(name: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 1024 || !name.is_ascii() || name.contains('\0') {
        return Err("unsupported guest asset path encoding or length".into());
    }
    let normalized = name.replace('\\', "/").to_ascii_lowercase();
    if normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!(
            "ambiguous guest asset path {name:?} (caller line {})",
            std::panic::Location::caller().line()
        ));
    }
    Ok(normalized)
}

impl GuestFiles {
    fn flush_console_repeats(&mut self) -> Result<(), String> {
        use std::io::Write;

        let tokens = self.standard_streams[1..]
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let mut output = std::io::stderr().lock();
        for token in tokens {
            if let Some(stream) = self.streams.get_mut(&token) {
                stream
                    .console_repeat
                    .flush(&mut output)
                    .map_err(|error| format!("guest console repeat summary failed: {error}"))?;
            }
        }
        output
            .flush()
            .map_err(|error| format!("guest console flush failed: {error}"))
    }

    fn record_directory_creation(&mut self, name: &str) {
        let now = std::time::SystemTime::now();
        let mut directory = Some(name);
        while let Some(path) = directory {
            self.directory_times
                .entry(path.to_owned())
                .or_insert([now; 3]);
            directory = path.rsplit_once('/').map(|(parent, _)| parent);
        }
        if let Some((parent, _)) = name.rsplit_once('/') {
            if let Some(times) = self.directory_times.get_mut(parent) {
                times[1] = now;
            }
        }
    }

    fn directory_exists(&self, name: &str) -> bool {
        let prefix = format!("{name}/");
        self.directories.contains(name)
            || self
                .directories
                .iter()
                .any(|path| path.starts_with(&prefix))
            || self.sources.keys().any(|path| path.starts_with(&prefix))
    }

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
                if let Some((parent, _)) = name.rsplit_once('/') {
                    // These are virtual mount directories; their birth is the
                    // namespace construction, not an inferred host directory.
                    files.record_directory_creation(parent);
                }
                if files.sources.insert(name, source).is_some() {
                    return Err("duplicate guest asset path".into());
                }
            }
            Ok(files)
        };
        read().map_err(|e| GuestError::Callback(format!("guest asset manifest: {e}")))
    }
}

impl GuestEngine<'static> {
    pub fn flush_guest_console_diagnostics(&mut self) -> Result<(), GuestError> {
        self.unicorn
            .get_data_mut()
            .guest_files
            .flush_console_repeats()
            .map_err(GuestError::Callback)
    }
}

fn open_guest_stream_with_share(
    unicorn: &mut Unicorn<'_, GuestState>,
    filename: &[u8],
    mode: &[u8],
    share_read_access: bool,
) -> Result<(u64, u32), String> {
    use std::io::Read;
    if filename.is_empty() || !valid_fopen_mode(mode) {
        return Ok((0, 22));
    }
    let trailing_separator = filename.ends_with(b"/") || filename.ends_with(b"\\");
    let absolute = canonical_guest_fullpath(filename)?;
    let filename = std::str::from_utf8(&absolute[..absolute.len() - 1])
        .map_err(|_| "unsupported fopen filename encoding")?;
    let name = guest_file_name(filename.trim_end_matches(['/', '\\']))?;
    let mode = trim_leading_crt_mode_spaces(mode);
    // The mounted namespace is read-only, including existing files opened r+.
    if mode[0] != b'r' || mode.contains(&b'+') || mode.contains(&b'D') {
        return Ok((0, 13));
    }

    if trailing_separator || unicorn.get_data().guest_files.directory_exists(&name) {
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
    if files.windows_files.values().any(|file| {
        file.name == name && (file.share & 1 == 0 || (!share_read_access && file.readable))
    }) || files.streams.values().any(|file| {
        file.name.as_deref() == Some(&name) && (!file.share_read_access || !share_read_access)
    }) {
        return Ok((0, 13));
    }
    if files.streams.len() + files.windows_files.len() >= 64
        || files.next_stream >= MAX_GUEST_STREAM_OPENS
    {
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
            name: Some(name.clone()),
            bytes,
            position: 0,
            readable: true,
            share_read_access,
            eof: false,
            buffer_state: None,
            fast_buffer: None,
            console_repeat: GuestConsoleRepeat::default(),
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

fn open_guest_stream(
    unicorn: &mut Unicorn<'_, GuestState>,
    filename: &[u8],
    mode: &[u8],
) -> Result<(u64, u32), String> {
    open_guest_stream_with_share(unicorn, filename, mode, true)
}

// Standard FILE objects have stable identities, including after fclose. The
// worker has no guest stdin input; stdout/stderr are output-only. Output calls
// remain explicit unsupported imports until their capture semantics are provided.
fn emulate_crt_stream_position(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: LegacyWin64Import,
) {
    let result = (|| -> Result<u64, String> {
        let stream = read_win64_import_argument(unicorn, 0)?;
        let Some(file) = unicorn.get_data().guest_files.streams.get(&stream) else {
            set_guest_crt_errno(unicorn, 9)?;
            return Ok(u64::MAX);
        };
        if operation == LegacyWin64Import::Ftelli64 {
            return Ok(file.position as u64);
        }
        if operation == LegacyWin64Import::Fgetpos {
            let output = read_win64_import_argument(unicorn, 1)?;
            if output == 0 || !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
                set_guest_crt_errno(unicorn, 22)?;
                return Ok(u32::MAX as u64);
            }
            unicorn
                .mem_write(output, &(file.position as i64).to_le_bytes())
                .map_err(|error| format!("fgetpos output write failed: {error}"))?;
            return Ok(0);
        }
        if operation == LegacyWin64Import::Fsetpos {
            let input = read_win64_import_argument(unicorn, 1)?;
            if input == 0 || !guest_range_has_permission(unicorn, input, 8, Prot::READ)? {
                set_guest_crt_errno(unicorn, 22)?;
                return Ok(u32::MAX as u64);
            }
            let bytes = unicorn
                .mem_read_as_vec(input, 8)
                .map_err(|error| format!("fsetpos input read failed: {error}"))?;
            let position = i64::from_le_bytes(bytes.try_into().unwrap());
            if position < 0 {
                set_guest_crt_errno(unicorn, 22)?;
                return Ok(u32::MAX as u64);
            }
            let file = unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&stream)
                .unwrap();
            file.position = position as usize;
            file.eof = false;
            return Ok(0);
        }
        if operation == LegacyWin64Import::Rewind {
            unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&stream)
                .unwrap()
                .position = 0;
            unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&stream)
                .unwrap()
                .eof = false;
            set_guest_crt_errno(unicorn, 0)?;
            return Ok(0);
        }
        let offset = read_win64_import_argument(unicorn, 1)? as i64;
        let origin = read_win64_import_argument(unicorn, 2)? as u32;
        let base = match origin {
            0 => 0i128,
            1 => file.position as i128,
            2 => file.bytes.len() as i128,
            _ => {
                set_guest_crt_errno(unicorn, 22)?;
                return Ok(u32::MAX as u64);
            }
        };
        let next = base + offset as i128;
        if !(0..=i64::MAX as i128).contains(&next) {
            set_guest_crt_errno(unicorn, 22)?;
            return Ok(u32::MAX as u64);
        }
        let file = unicorn
            .get_data_mut()
            .guest_files
            .streams
            .get_mut(&stream)
            .unwrap();
        file.position = next as usize;
        file.eof = false;
        Ok(0)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_crt_descriptor(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        if let LegacyWin64Import::CrtOpen(wide) = operation {
            let path = read_win64_import_argument(unicorn, 0)?;
            let flags = read_win64_import_argument(unicorn, 1)? as u32;
            if path == 0 || flags & 0x703 != 0 {
                set_guest_crt_errno(unicorn, if path == 0 { 22 } else { 13 })?;
                return Ok(u32::MAX as u64);
            }
            let name = if wide {
                read_guest_wide_file_string(unicorn, path, 4096, "_wopen path")?.into_bytes()
            } else {
                read_crt_stdio_c_string(unicorn, path, 4096, "_open path")?
            };
            let binary = flags & 0x8000 != 0;
            // Preserve raw bytes because _setmode may switch a descriptor to
            // binary before its first read. Text translation belongs in _read.
            let (token, errno) = open_guest_stream(unicorn, &name, b"rb")?;
            if token == 0 {
                set_guest_crt_errno(unicorn, errno)?;
                return Ok(u32::MAX as u64);
            }
            let fd = unicorn.get_data().guest_files.next_descriptor.max(3);
            if fd >= 4096 || unicorn.get_data().guest_files.descriptors.len() >= 64 {
                unicorn
                    .mem_unmap(token, PAGE_SIZE)
                    .map_err(|error| format!("descriptor FILE token cleanup failed: {error}"))?;
                let files = &mut unicorn.get_data_mut().guest_files;
                let stream = files.streams.remove(&token).unwrap();
                files.live_bytes -= stream.bytes.len();
                set_guest_crt_errno(unicorn, 24)?;
                return Ok(u32::MAX as u64);
            }
            let stream = unicorn
                .get_data_mut()
                .guest_files
                .streams
                .remove(&token)
                .unwrap();
            unicorn
                .mem_unmap(token, PAGE_SIZE)
                .map_err(|error| format!("descriptor FILE token unmap failed: {error}"))?;
            let files = &mut unicorn.get_data_mut().guest_files;
            files.next_descriptor = fd + 1;
            files.descriptors.insert(fd, stream);
            files
                .descriptor_modes
                .insert(fd, if binary { 0x8000 } else { 0x4000 });
            return Ok(fd as u64);
        }
        let fd = read_win64_import_argument(unicorn, 0)? as u32 as i32;
        match operation {
            LegacyWin64Import::CrtRead => {
                let output = read_win64_import_argument(unicorn, 1)?;
                let count = read_win64_import_argument(unicorn, 2)? as u32 as usize;
                let stream = match unicorn.get_data().guest_files.descriptors.get(&fd) {
                    Some(stream) => stream,
                    None => {
                        set_guest_crt_errno(unicorn, 9)?;
                        return Ok(u32::MAX as u64);
                    }
                };
                let actual = count.min(stream.bytes.len().saturating_sub(stream.position));
                if actual != 0 {
                    if output == 0
                        || !guest_range_has_permission(unicorn, output, actual as u64, Prot::WRITE)?
                    {
                        return Err("_read output is not writable".into());
                    }
                    let bytes = stream.bytes[stream.position..stream.position + actual].to_vec();
                    unicorn
                        .mem_write(output, &bytes)
                        .map_err(|error| format!("_read output failed: {error}"))?;
                }
                let stream = unicorn
                    .get_data_mut()
                    .guest_files
                    .descriptors
                    .get_mut(&fd)
                    .unwrap();
                stream.position += actual;
                stream.eof = actual < count;
                Ok(actual as u64)
            }
            LegacyWin64Import::CrtClose => {
                let Some(stream) = unicorn.get_data_mut().guest_files.descriptors.remove(&fd)
                else {
                    set_guest_crt_errno(unicorn, 9)?;
                    return Ok(u32::MAX as u64);
                };
                let files = &mut unicorn.get_data_mut().guest_files;
                files.descriptor_modes.remove(&fd);
                files.live_bytes -= stream.bytes.len();
                Ok(0)
            }
            LegacyWin64Import::CrtLseek64 => {
                let offset = read_win64_import_argument(unicorn, 1)? as i64;
                let origin = read_win64_import_argument(unicorn, 2)? as u32;
                let Some(stream) = unicorn.get_data_mut().guest_files.descriptors.get_mut(&fd)
                else {
                    set_guest_crt_errno(unicorn, 9)?;
                    return Ok(u64::MAX);
                };
                let base = match origin {
                    0 => 0i128,
                    1 => stream.position as i128,
                    2 => stream.bytes.len() as i128,
                    _ => {
                        set_guest_crt_errno(unicorn, 22)?;
                        return Ok(u64::MAX);
                    }
                };
                let next = base + offset as i128;
                if !(0..=i64::MAX as i128).contains(&next) {
                    set_guest_crt_errno(unicorn, 22)?;
                    return Ok(u64::MAX);
                }
                stream.position = next as usize;
                stream.eof = false;
                Ok(next as u64)
            }
            LegacyWin64Import::CrtSetMode => {
                let mode = read_win64_import_argument(unicorn, 1)? as u32 as i32;
                if !matches!(mode, 0x4000 | 0x8000) {
                    set_guest_crt_errno(unicorn, 22)?;
                    return Ok(u32::MAX as u64);
                }
                if fd < 0
                    || (fd > 2 && !unicorn.get_data().guest_files.descriptors.contains_key(&fd))
                {
                    set_guest_crt_errno(unicorn, 9)?;
                    return Ok(u32::MAX as u64);
                }
                Ok(unicorn
                    .get_data_mut()
                    .guest_files
                    .descriptor_modes
                    .insert(fd, mode)
                    .unwrap_or(0x4000) as u32 as u64)
            }
            _ => Err("invalid CRT descriptor operation".into()),
        }
    })();
    finish_guest_stdio(unicorn, result);
}

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
        if files.streams.len() + files.windows_files.len() >= 64
            || files.next_stream >= MAX_GUEST_STREAM_OPENS
        {
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
                name: None,
                bytes: Vec::new(),
                position: 0,
                readable: index == 0,
                share_read_access: true,
                eof: false,
                buffer_state: None,
                fast_buffer: None,
                console_repeat: GuestConsoleRepeat::default(),
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
        let mut values = [0u8; 20];
        unicorn
            .mem_read(state, &mut values)
            .map_err(|error| format!("FILE buffer state read: {error}"))?;
        let modeled_fast_buffer = stream.fast_buffer.is_some_and(|buffer| {
            let cursor = u64::from_le_bytes(values[8..16].try_into().unwrap());
            let remaining = u32::from_le_bytes(values[16..20].try_into().unwrap()) as u64;
            unicorn
                .get_data()
                .crt_heap
                .regular_allocation(buffer)
                .is_ok_and(|allocation| {
                    let end = buffer.saturating_add(allocation.requested_size);
                    values[..16] == [0; 16]
                        || (values[..8] == [0; 8]
                            && (buffer..=end).contains(&cursor)
                            && cursor.checked_add(remaining) == Some(end))
                })
        });
        if values != [0; 20] && !modeled_fast_buffer {
            let fast_allocation = stream.fast_buffer.map(|buffer| {
                unicorn
                    .get_data()
                    .crt_heap
                    .regular_allocation(buffer)
                    .map(|allocation| allocation.requested_size)
            });
            return Err(format!(
                "guest-modified FILE buffering state is not implemented: state={} fast_buffer={:?} fast_allocation={fast_allocation:?}",
                values
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                stream.fast_buffer
            ));
        }
    }
    Ok(())
}

fn emulate_fopen(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let name = read_win64_import_argument(unicorn, 0)?;
        let mode = read_win64_import_argument(unicorn, 1)?;
        if name == 0 || mode == 0 {
            set_guest_crt_errno(unicorn, 22)?;
            return Ok(0);
        }
        let name = read_crt_stdio_c_string(unicorn, name, MAX_CRT_STRING_BYTES, "fopen filename")?;
        let mode = read_crt_stdio_c_string(unicorn, mode, 64, "fopen mode")?;
        let (stream, errno) = open_guest_stream(unicorn, &name, &mode)?;
        if errno != 0 {
            set_guest_crt_errno(unicorn, errno)?;
        }
        Ok(stream)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_guest_stdio(unicorn: &mut Unicorn<'_, GuestState>, import: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        if import == LegacyWin64Import::Fflush {
            use std::io::Write;
            let token = read_win64_import_argument(unicorn, 0)?;
            let tokens = if token == 0 {
                unicorn
                    .get_data()
                    .guest_files
                    .streams
                    .keys()
                    .copied()
                    .collect::<Vec<_>>()
            } else {
                vec![token]
            };
            for &token in &tokens {
                if !unicorn.get_data().guest_files.streams.contains_key(&token) {
                    return Err("fflush received stale or foreign FILE".into());
                }
                require_unbuffered_guest_stream(unicorn, token)?;
            }
            let mut output = std::io::stderr().lock();
            output
                .flush()
                .map_err(|e| format!("guest console flush failed: {e}"))?;
            return Ok(0);
        }
        if import == LegacyWin64Import::Fwrite {
            return guest_fwrite(unicorn);
        }
        if import == LegacyWin64Import::Feof {
            let token = read_win64_import_argument(unicorn, 0)?;
            require_unbuffered_guest_stream(unicorn, token)?;
            return Ok(u64::from(
                unicorn
                    .get_data()
                    .guest_files
                    .streams
                    .get(&token)
                    .ok_or("feof received stale or foreign FILE")?
                    .eof,
            ));
        }
        if matches!(
            import,
            LegacyWin64Import::Ferror | LegacyWin64Import::LockFile | LegacyWin64Import::UnlockFile
        ) {
            let token = read_win64_import_argument(unicorn, 0)?;
            require_unbuffered_guest_stream(unicorn, token)?;
            return Ok(0);
        }
        if import == LegacyWin64Import::Fgetc {
            let token = read_win64_import_argument(unicorn, 0)?;
            require_unbuffered_guest_stream(unicorn, token)?;
            let expected_return = unicorn.get_data().sapphire_filebuf_fgetc_return;
            if let Some(expected_return) = expected_return {
                let rsp = unicorn
                    .reg_read(RegisterX86::RSP)
                    .map_err(|error| format!("read Sapphire filebuf return stack: {error}"))?;
                let mut return_bytes = [0u8; 8];
                unicorn
                    .mem_read(rsp, &mut return_bytes)
                    .map_err(|error| format!("read Sapphire filebuf return address: {error}"))?;
                let stream = unicorn
                    .get_data()
                    .guest_files
                    .streams
                    .get(&token)
                    .ok_or("getc received stale or foreign FILE")?;
                if u64::from_le_bytes(return_bytes) == expected_return
                    && stream.fast_buffer.is_none()
                    && stream.bytes.len().saturating_sub(stream.position) > PAGE_SIZE as usize
                {
                    let first = stream.bytes[stream.position];
                    let remaining = stream.bytes[stream.position + 1..].to_vec();
                    let filebuf = unicorn
                        .reg_read(RegisterX86::RSI)
                        .map_err(|error| format!("read Sapphire filebuf object: {error}"))?;
                    let mut pointer_cells = [0u8; 16];
                    unicorn
                        .mem_read(filebuf + 0x38, &mut pointer_cells[..8])
                        .map_err(|error| format!("read Sapphire filebuf pointer cell: {error}"))?;
                    unicorn
                        .mem_read(filebuf + 0x50, &mut pointer_cells[8..])
                        .map_err(|error| format!("read Sapphire filebuf count cell: {error}"))?;
                    let pointer_cell = u64::from_le_bytes(pointer_cells[..8].try_into().unwrap());
                    let count_cell = u64::from_le_bytes(pointer_cells[8..].try_into().unwrap());
                    if pointer_cell != 0
                        && count_cell != 0
                        && guest_range_has_permission(unicorn, pointer_cell, 8, Prot::WRITE)?
                        && guest_range_has_permission(unicorn, count_cell, 4, Prot::WRITE)?
                    {
                        let buffer = allocate_crt_region(unicorn, remaining.len() as u64)
                            .map_err(|error| error.to_string())?;
                        unicorn
                            .mem_write(buffer, &remaining)
                            .map_err(|error| format!("write Sapphire filebuf bytes: {error}"))?;
                        unicorn
                            .mem_write(pointer_cell, &buffer.to_le_bytes())
                            .map_err(|error| format!("write Sapphire filebuf pointer: {error}"))?;
                        unicorn
                            .mem_write(count_cell, &(remaining.len() as i32).to_le_bytes())
                            .map_err(|error| format!("write Sapphire filebuf count: {error}"))?;
                        let stream = unicorn
                            .get_data_mut()
                            .guest_files
                            .streams
                            .get_mut(&token)
                            .unwrap();
                        stream.position = stream.bytes.len();
                        stream.fast_buffer = Some(buffer);
                        return Ok(u64::from(first));
                    }
                }
            }
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
                None => {
                    stream.eof = true;
                    u64::from(u32::MAX)
                } // EOF is an int, not a signed byte.
            });
        }
        if import == LegacyWin64Import::Ungetc {
            let character = read_win64_import_argument(unicorn, 0)? as u32;
            let token = read_win64_import_argument(unicorn, 1)?;
            require_unbuffered_guest_stream(unicorn, token)?;
            let stream = unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&token)
                .ok_or("ungetc received stale or foreign FILE")?;
            if character == u32::MAX || stream.position == 0 {
                return Ok(u64::from(u32::MAX));
            }
            let byte = character as u8;
            if stream.bytes[stream.position - 1] != byte {
                return Ok(u64::from(u32::MAX));
            }
            stream.position -= 1;
            stream.eof = false;
            return Ok(u64::from(byte));
        }
        if import == LegacyWin64Import::Fgets {
            let output = read_win64_import_argument(unicorn, 0)?;
            let count = read_win64_import_argument(unicorn, 1)? as u32 as i32;
            let token = read_win64_import_argument(unicorn, 2)?;
            if output == 0 || count <= 0 {
                set_guest_crt_errno(unicorn, 22)?;
                return Ok(0);
            }
            require_unbuffered_guest_stream(unicorn, token)?;
            let stream = unicorn
                .get_data()
                .guest_files
                .streams
                .get(&token)
                .ok_or("fgets received stale or foreign FILE")?;
            if !stream.readable {
                set_guest_crt_errno(unicorn, 9)?;
                return Ok(0);
            }
            if stream.position >= stream.bytes.len() && count > 1 {
                unicorn
                    .get_data_mut()
                    .guest_files
                    .streams
                    .get_mut(&token)
                    .unwrap()
                    .eof = true;
                return Ok(0);
            }
            let maximum = (count as usize).saturating_sub(1);
            let available = &stream.bytes[stream.position..];
            let actual = available
                .iter()
                .take(maximum)
                .position(|byte| *byte == b'\n')
                .map_or(maximum.min(available.len()), |position| position + 1);
            if !guest_range_has_permission(unicorn, output, actual as u64 + 1, Prot::WRITE)? {
                return Err("fgets output is not writable".into());
            }
            let mut bytes = stream.bytes[stream.position..stream.position + actual].to_vec();
            bytes.push(0);
            unicorn
                .mem_write(output, &bytes)
                .map_err(|error| format!("fgets output write failed: {error}"))?;
            let stream = unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&token)
                .unwrap();
            stream.position += actual;
            stream.eof = false;
            return Ok(output);
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
            if let Some(buffer) = unicorn.get_data().guest_files.streams[&token].fast_buffer {
                free_crt_region(unicorn, buffer)?;
            }
            if unicorn.get_data().guest_files.standard_streams[1..].contains(&Some(token)) {
                unicorn
                    .get_data_mut()
                    .guest_files
                    .streams
                    .get_mut(&token)
                    .unwrap()
                    .console_repeat
                    .flush(&mut std::io::stderr().lock())
                    .map_err(|error| format!("guest console repeat summary failed: {error}"))?;
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
        let actual = (wanted as usize).min(stream.bytes.len().saturating_sub(stream.position));
        let hit_eof = actual < wanted as usize;
        if actual == 0 {
            unicorn
                .get_data_mut()
                .guest_files
                .streams
                .get_mut(&token)
                .unwrap()
                .eof = hit_eof;
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
        unicorn
            .get_data_mut()
            .guest_files
            .streams
            .get_mut(&token)
            .unwrap()
            .eof = hit_eof;
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

fn canonical_guest_search_name(query: &str) -> Result<String, String> {
    let mut path = canonical_guest_fullpath(query.as_bytes())?;
    path.pop();
    guest_file_name(std::str::from_utf8(&path).unwrap())
}

fn guest_find_records(files: &GuestFiles, query: &str) -> Result<Vec<[u8; 320]>, String> {
    let query = canonical_guest_search_name(query)?;
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
    for name in &files.directories {
        if let Some(rest) = name.strip_prefix(&prefix) {
            let leaf = rest.split('/').next().unwrap();
            if guest_star_match(pattern.as_bytes(), leaf.as_bytes()) {
                candidates.entry(leaf.to_string()).or_insert(None);
            }
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
            let attributes = files
                .directory_attributes
                .get(&format!("{prefix}{name}"))
                .copied()
                .unwrap_or_default();
            record[..4].copy_from_slice(&(0x10u32 | (attributes & !128)).to_le_bytes());
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
            let normalized = canonical_guest_search_name(query)?;
            let directory = normalized
                .rsplit_once('/')
                .ok_or("relative file search is unsupported")?
                .0;
            if !files.directory_exists(directory) {
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

const MAX_GUEST_DIRECTORIES: usize = 4096;

fn create_guest_directory(
    unicorn: &mut Unicorn<'_, GuestState>,
    wide: bool,
) -> Result<u64, String> {
    let pointer = read_win64_import_argument(unicorn, 0)?;
    let attributes = read_win64_import_argument(unicorn, 1)?;
    let fail = |unicorn: &mut Unicorn<'_, GuestState>, error| {
        unicorn.get_data_mut().windows_last_error = error;
        Ok(0)
    };
    if pointer == 0 {
        return fail(unicorn, 87);
    }
    let bytes;
    let wide_text;
    let text = if wide {
        wide_text = read_guest_wide_file_string(unicorn, pointer, 260, "CreateDirectoryW path")?;
        wide_text.as_str()
    } else {
        bytes = read_crt_stdio_c_string(unicorn, pointer, 260, "CreateDirectoryA path")?;
        std::str::from_utf8(&bytes).map_err(|_| "unsupported directory path encoding")?
    };
    if text == "\\" || text == "/" {
        return fail(unicorn, 183);
    }
    if text.chars().any(|character| character <= '\u{1f}') {
        return fail(unicorn, 123); // ERROR_INVALID_NAME
    }
    // A leading separator is rooted on the guest's C: drive. Keep this in
    // the session-only namespace; never resolve it against the host filesystem.
    let rooted;
    let path =
        if text.starts_with(['\\', '/']) && !text.starts_with("\\\\") && !text.starts_with("//") {
            rooted = format!("C:{text}");
            rooted.trim_end_matches(['\\', '/'])
        } else {
            text.trim_end_matches(['\\', '/'])
        };
    let name = guest_file_name(path)?;
    if name.len() < 4
        || name.as_bytes()[1..3] != *b":/"
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || name[3..].contains(['*', '?', ':', '<', '>', '|', '"'])
        || name.split('/').any(|part| part.ends_with(['.', ' ']))
    {
        return Err("CreateDirectoryA requires an unambiguous absolute guest path".into());
    }
    let (parent, _) = name.rsplit_once('/').ok_or("directory has no parent")?;
    let files = &unicorn.get_data().guest_files;
    if files.directory_exists(&name) || files.sources.contains_key(&name) {
        return fail(unicorn, 183);
    }
    if parent != "c:" && !files.directory_exists(parent) {
        return fail(unicorn, 3);
    }
    // Only session-created directories accept children. Mounted assets remain
    // read-only, including their implicit parent directories.
    if parent != "c:" && !files.directories.contains(parent) {
        return fail(unicorn, 5);
    }
    if files.directories.len() >= MAX_GUEST_DIRECTORIES {
        return fail(unicorn, 8);
    }
    let mut dacl = None;
    if attributes != 0 {
        let sa = acl_read(unicorn, attributes, 24)?;
        if u32::from_le_bytes(sa[..4].try_into().unwrap()) != 24 {
            return fail(unicorn, 87);
        }
        let sd = u64::from_le_bytes(sa[8..16].try_into().unwrap());
        // bInheritHandle has no effect: directory creation returns no handle.
        if sd != 0 {
            let descriptor = acl_read(unicorn, sd, 40)?;
            let control = u16::from_le_bytes([descriptor[2], descriptor[3]]);
            if descriptor[0] != 1 {
                return fail(unicorn, 1305);
            }
            if control & !0x000c != 0 || descriptor[8..32].iter().any(|b| *b != 0) {
                return Err(
                    "CreateDirectoryA unsupported security descriptor owner/group/SACL/control"
                        .into(),
                );
            }
            if control & 4 != 0 {
                let address = u64::from_le_bytes(descriptor[32..40].try_into().unwrap());
                if address != 0 {
                    let header = acl_read(unicorn, address, 8)?;
                    let size = u16::from_le_bytes([header[2], header[3]]) as u64;
                    if !(2..=4).contains(&header[0]) || size < 8 {
                        return fail(unicorn, 1336);
                    }
                    let acl = acl_read(unicorn, address, size)?;
                    // The current session namespace supports an unrestricted
                    // inheritable Everyone grant. Restrictive or other-token
                    // policies require access checking before we can accept them.
                    if acl[4..6] != [1, 0]
                        || acl.len() < 28
                        || acl[8..12] != [0, 3, 20, 0]
                        || acl[12..16] != 0x10000000u32.to_le_bytes()
                        || acl[16..28] != [1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]
                    {
                        return Err("CreateDirectoryA unsupported DACL access policy".into());
                    }
                    dacl = Some(acl);
                }
            } else {
                dacl = unicorn
                    .get_data()
                    .guest_files
                    .directory_dacls
                    .get(parent)
                    .map(inherit_guest_directory_dacl);
            }
        } else {
            dacl = unicorn
                .get_data()
                .guest_files
                .directory_dacls
                .get(parent)
                .map(inherit_guest_directory_dacl);
        }
    } else {
        dacl = unicorn
            .get_data()
            .guest_files
            .directory_dacls
            .get(parent)
            .map(inherit_guest_directory_dacl);
    }
    let files = &mut unicorn.get_data_mut().guest_files;
    if let Some(dacl) = dacl {
        files.directory_dacls.insert(name.clone(), dacl);
    }
    files.record_directory_creation(&name);
    files.directories.insert(name);
    Ok(1)
}

fn set_guest_file_attributes_a(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let path = read_win64_import_argument(unicorn, 0)?;
    let attributes = read_win64_import_argument(unicorn, 1)? as u32;
    if path == 0 {
        unicorn.get_data_mut().windows_last_error = 87;
        return Ok(0);
    }
    let bytes = read_crt_stdio_c_string(unicorn, path, 260, "SetFileAttributesA path")?;
    let absolute = canonical_guest_fullpath(&bytes)?;
    let text = std::str::from_utf8(&absolute[..absolute.len() - 1])
        .map_err(|_| "SetFileAttributesA path encoding is unsupported")?;
    let name = guest_file_name(text.trim_end_matches(['/', '\\']))?;
    const SUPPORTED: u32 = 1 | 2 | 4 | 32 | 128 | 4096 | 8192;
    if attributes == 0 || attributes & !SUPPORTED != 0 || attributes & 128 != 0 && attributes != 128
    {
        return Err("SetFileAttributesA requested unsupported attributes".into());
    }
    let files = &unicorn.get_data().guest_files;
    if files.sources.contains_key(&name) {
        unicorn.get_data_mut().windows_last_error = 5;
        return Ok(0);
    }
    if !files.directory_exists(&name) {
        let parent = name
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        unicorn.get_data_mut().windows_last_error =
            if files.directory_exists(parent) { 2 } else { 3 };
        return Ok(0);
    }
    if !files.directories.contains(&name) {
        unicorn.get_data_mut().windows_last_error = 5;
        return Ok(0);
    }
    unicorn
        .get_data_mut()
        .guest_files
        .directory_attributes
        .insert(name, attributes);
    Ok(1)
}

fn inherit_guest_directory_dacl(parent: &Vec<u8>) -> Vec<u8> {
    let mut inherited = parent.clone();
    // Stored policies have one validated inheritable access-allowed ACE.
    inherited[9] |= 0x10; // INHERITED_ACE
    inherited
}

// Windows _stat64i32: 32-bit dev/size, 16-bit inode/mode/link/uid/gid,
// padding at 14..16, and three 64-bit times at 24, 32, 40.
fn guest_stat64i32(unicorn: &mut Unicorn<'_, GuestState>, wide: bool) -> Result<u64, String> {
    let path = read_win64_import_argument(unicorn, 0)?;
    let output = read_win64_import_argument(unicorn, 1)?;
    if path == 0 || output == 0 {
        return Err("_wstat64i32 requires an invalid parameter handler for null arguments".into());
    }
    if !guest_range_has_permission(unicorn, output, 48, Prot::WRITE)? {
        return Err("_wstat64i32 output is not fully writable".into());
    }
    let text = if wide {
        let mut units = Vec::new();
        for index in 0..=1024u64 {
            let address = path
                .checked_add(index * 2)
                .ok_or("_wstat64i32 path overflow")?;
            if !guest_range_has_permission(unicorn, address, 2, Prot::READ)? {
                return Err("_wstat64i32 path is not readable".into());
            }
            let bytes = unicorn
                .mem_read_as_vec(address, 2)
                .map_err(|e| e.to_string())?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                break;
            }
            if index == 1024 {
                return Err("_wstat64i32 path exceeds supported bound".into());
            }
            units.push(unit);
        }
        String::from_utf16(&units).map_err(|_| "_wstat64i32 invalid UTF-16")?
    } else {
        let bytes = read_crt_stdio_c_string(unicorn, path, 2049, "_stat64i32 path")?;
        let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
        if invalid {
            return Err("_stat64i32 invalid CP932 path".into());
        }
        text.into_owned()
    };
    let normalized = if text.is_ascii() {
        let mut path = canonical_guest_fullpath(text.as_bytes())?;
        path.pop(); // remove the NUL used by the `_fullpath` ABI
        String::from_utf8(path).unwrap()
    } else {
        return Err("_wstat64i32 non-ASCII path normalization is not implemented".into());
    };
    let mut record = [0u8; 48];
    let result = guest_stat_record(&unicorn.get_data().guest_files, &normalized, &mut record)?;
    unicorn
        .mem_write(output, &record)
        .map_err(|e| e.to_string())?;
    if result != 0 {
        set_guest_crt_errno(unicorn, result)?;
        Ok(u32::MAX as u64)
    } else {
        Ok(0)
    }
}

fn guest_stat_record(files: &GuestFiles, text: &str, record: &mut [u8; 48]) -> Result<u32, String> {
    if text.is_empty() {
        return Ok(2);
    }
    let trailing_slash = text.ends_with(['/', '\\']);
    let name = guest_file_name(text.trim_end_matches(['/', '\\']))?;
    if name.len() < 2
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || name.as_bytes()[1] != b':'
        || (name.len() > 2 && name.as_bytes()[2] != b'/')
    {
        return Err("_wstat64i32 requires an absolute guest drive path".into());
    }
    if name.len() == 2 && !trailing_slash {
        return Err("_wstat64i32 drive-relative path is unsupported".into());
    }
    if name[2..].contains(['*', '?', ':', '<', '>', '|', '"']) {
        return Ok(2);
    }
    let (mode, size, times) = if let Some(source) = files.sources.get(&name) {
        if trailing_slash {
            return Ok(2);
        }
        let metadata = match std::fs::metadata(source) {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(2),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(13),
            Err(e) => return Err(format!("_wstat64i32 mounted metadata: {e}")),
        };
        if !metadata.is_file() {
            return Err("_wstat64i32 mounted source is not a regular file".into());
        }
        if metadata.len() > i32::MAX as u64 {
            return Ok(132);
        } // UCRT EOVERFLOW
        let executable = [".exe", ".cmd", ".bat", ".com"]
            .iter()
            .any(|suffix| name.ends_with(suffix));
        let mut times = [std::time::UNIX_EPOCH; 3];
        for (index, value) in [metadata.accessed(), metadata.modified(), metadata.created()]
            .into_iter()
            .enumerate()
        {
            times[index] =
                value.map_err(|e| format!("_wstat64i32 unavailable mounted timestamp: {e}"))?;
        }
        (
            0x8000u16 | if executable { 0o555 } else { 0o444 },
            metadata.len() as i32,
            times,
        )
    } else if files.directory_exists(&name) {
        let times = *files
            .directory_times
            .get(&name)
            .ok_or("_wstat64i32 missing guest directory metadata")?;
        (
            0x4000u16
                | if files.directories.contains(&name) {
                    0o777
                } else {
                    0o555
                },
            0,
            times,
        )
    } else {
        return Ok(2);
    };
    let drive = (name.as_bytes()[0] - b'a') as u32;
    record[0..4].copy_from_slice(&drive.to_le_bytes());
    record[6..8].copy_from_slice(&mode.to_le_bytes());
    record[8..10].copy_from_slice(&1u16.to_le_bytes());
    record[16..20].copy_from_slice(&drive.to_le_bytes());
    record[20..24].copy_from_slice(&size.to_le_bytes());
    for (index, time) in times.into_iter().enumerate() {
        let seconds = time
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "_wstat64i32 timestamp predates supported CRT epoch")?
            .as_secs();
        if seconds > 32535215999 {
            return Err("_wstat64i32 timestamp exceeds CRT range".into());
        }
        record[24 + index * 8..32 + index * 8].copy_from_slice(&(seconds as i64).to_le_bytes());
    }
    Ok(0)
}

// The guest process starts at the root of C:. Host cwd and per-drive host
// environment variables never participate in guest path resolution.
const GUEST_INITIAL_CURRENT_DIRECTORY: &str = "C:\\";

fn canonical_guest_fullpath(path: &[u8]) -> Result<Vec<u8>, String> {
    if !path.is_ascii() || path.contains(&0) {
        return Err("_fullpath unsupported path encoding".into());
    }
    let text = std::str::from_utf8(path).unwrap().replace('/', "\\");
    if text.starts_with("\\\\") {
        return Err("_fullpath UNC/device namespaces are not implemented".into());
    }
    let bytes = text.as_bytes();
    let (drive, rest) = if bytes.len() >= 2 && bytes[1] == b':' {
        if !bytes[0].is_ascii_alphabetic() {
            return Err("_fullpath invalid drive prefix".into());
        }
        if bytes.len() > 2 && bytes[2] == b'\\' {
            (bytes[0] as char, &text[3..])
        } else if bytes[0].eq_ignore_ascii_case(&b'c') {
            (bytes[0] as char, &text[2..])
        } else {
            return Err("_fullpath current directory on another drive is not configured".into());
        }
    } else {
        (
            GUEST_INITIAL_CURRENT_DIRECTORY.as_bytes()[0] as char,
            text.trim_start_matches('\\'),
        )
    };
    let trailing_separator = text.ends_with('\\');
    let mut parts = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => {
                // Windows trims dots/spaces and recognizes DOS devices. Until
                // those rules are modeled, stop rather than resolve another name.
                let stem = part.split('.').next().unwrap().to_ascii_uppercase();
                let device = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
                    || (stem.len() == 4
                        && (stem.starts_with("COM") || stem.starts_with("LPT"))
                        && matches!(stem.as_bytes()[3], b'1'..=b'9'));
                if part.ends_with(['.', ' '])
                    || part.starts_with(' ')
                    || device
                    || part.contains([':', '<', '>', '|', '"'])
                    || part.bytes().any(|b| b < 32)
                {
                    return Err("_fullpath unsupported ambiguous or device path component".into());
                }
                parts.push(part);
            }
        }
    }
    let mut result = format!("{drive}:\\{}", parts.join("\\"));
    if trailing_separator && !result.ends_with('\\') {
        result.push('\\');
    }
    result.push('\0');
    Ok(result.into_bytes())
}

fn guest_fullpath(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let output = read_win64_import_argument(unicorn, 0)?;
    let input = read_win64_import_argument(unicorn, 1)?;
    let capacity = read_win64_import_argument(unicorn, 2)?;
    let path = if input == 0 {
        Vec::new()
    } else {
        read_crt_stdio_c_string(unicorn, input, 1024, "_fullpath source")?
    };
    if output != 0 && capacity == 0 {
        return Err("_fullpath requires an invalid parameter handler for zero capacity".into());
    }
    let result = canonical_guest_fullpath(&path)?;
    let required = result.len() as u64;
    if output != 0 && capacity < required {
        set_guest_crt_errno(unicorn, 34)?; // ERANGE
        return Ok(0);
    }
    let allocated = output == 0;
    let destination = if allocated {
        // Nonempty-path _fullpath ignores maxLength when allocating. Empty
        // paths delegate to getcwd, whose requested capacity must also fit.
        if path.is_empty() && capacity != 0 && capacity < required {
            set_guest_crt_errno(unicorn, 34)?;
            return Ok(0);
        }
        match allocate_crt_region(
            unicorn,
            if path.is_empty() {
                capacity.max(required)
            } else {
                required
            },
        ) {
            Ok(pointer) => pointer,
            Err(_) => {
                set_guest_crt_errno(unicorn, 12)?;
                return Ok(0);
            }
        }
    } else {
        if !guest_range_has_permission(unicorn, output, required, Prot::WRITE)? {
            return Err("_fullpath output is not fully writable".into());
        }
        output
    };
    if let Err(error) = unicorn.mem_write(destination, &result) {
        if allocated {
            free_crt_region(unicorn, destination)?;
        }
        return Err(format!("_fullpath output write: {error}"));
    }
    Ok(destination)
}

fn read_guest_wide_file_string(
    unicorn: &Unicorn<'_, GuestState>,
    pointer: u64,
    limit: u64,
    label: &str,
) -> Result<String, String> {
    let mut units = Vec::new();
    for index in 0..=limit {
        let address = pointer
            .checked_add(index.checked_mul(2).ok_or("wide string length overflow")?)
            .ok_or("wide string address overflow")?;
        if !guest_range_has_permission(unicorn, address, 2, Prot::READ)? {
            return Err(format!("{label} is not readable"));
        }
        let bytes = unicorn
            .mem_read_as_vec(address, 2)
            .map_err(|e| e.to_string())?;
        let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
        if unit == 0 {
            return String::from_utf16(&units).map_err(|_| format!("{label} invalid UTF-16"));
        }
        if index == limit {
            return Err(format!("{label} exceeds supported bound"));
        }
        units.push(unit);
    }
    unreachable!()
}

fn emulate_wfopen(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let name = read_win64_import_argument(unicorn, 0)?;
        let mode = read_win64_import_argument(unicorn, 1)?;
        if name == 0 || mode == 0 {
            return Err("_wfopen requires an invalid parameter handler for null arguments".into());
        }
        let name = read_guest_wide_file_string(unicorn, name, 1024, "_wfopen filename")?;
        let mode = read_guest_wide_file_string(unicorn, mode, 64, "_wfopen mode")?;
        if !mode.is_ascii() {
            return Err("_wfopen unsupported mode encoding".into());
        }
        let (stream, errno) = open_guest_stream(unicorn, name.as_bytes(), mode.as_bytes())?;
        if errno != 0 {
            set_guest_crt_errno(unicorn, errno)?;
        }
        Ok(stream)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_wfsopen(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let name = read_win64_import_argument(unicorn, 0)?;
        let mode = read_win64_import_argument(unicorn, 1)?;
        let share = read_win64_import_argument(unicorn, 2)? as u32;
        if name == 0 || mode == 0 {
            return Err("_wfsopen requires an invalid parameter handler for null arguments".into());
        }
        let share_read_access = match share {
            0x10 | 0x30 => false,
            0x20 | 0x40 | 0x80 => true,
            _ => {
                set_guest_crt_errno(unicorn, 22)?;
                return Ok(0);
            }
        };
        let name = read_guest_wide_file_string(unicorn, name, 1024, "_wfsopen filename")?;
        let mode = read_guest_wide_file_string(unicorn, mode, 64, "_wfsopen mode")?;
        if !mode.is_ascii() {
            return Err("_wfsopen unsupported mode encoding".into());
        }
        let (stream, errno) = open_guest_stream_with_share(
            unicorn,
            name.as_bytes(),
            mode.as_bytes(),
            share_read_access,
        )?;
        if errno != 0 {
            set_guest_crt_errno(unicorn, errno)?;
        }
        Ok(stream)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_fsopen(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let name = read_win64_import_argument(unicorn, 0)?;
        let mode = read_win64_import_argument(unicorn, 1)?;
        let share = read_win64_import_argument(unicorn, 2)? as u32;
        if name == 0 || mode == 0 {
            return Err("_fsopen requires an invalid parameter handler for null arguments".into());
        }
        let share_read_access = match share {
            0x10 | 0x30 => false,
            0x20 | 0x40 | 0x80 => true,
            _ => {
                set_guest_crt_errno(unicorn, 22)?;
                return Ok(0);
            }
        };
        let name =
            read_crt_stdio_c_string(unicorn, name, MAX_CRT_STRING_BYTES, "_fsopen filename")?;
        let mode = read_crt_stdio_c_string(unicorn, mode, 64, "_fsopen mode")?;
        let (stream, errno) =
            open_guest_stream_with_share(unicorn, &name, &mode, share_read_access)?;
        if errno != 0 {
            set_guest_crt_errno(unicorn, errno)?;
        }
        Ok(stream)
    })();
    finish_guest_stdio(unicorn, result);
}

fn guest_volume_information(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let root = read_win64_import_argument(unicorn, 0)?;
    let path = if root == 0 {
        GUEST_INITIAL_CURRENT_DIRECTORY.as_bytes().to_vec()
    } else {
        read_crt_stdio_c_string(unicorn, root, 260, "GetVolumeInformationA root")?
    };
    if path.len() != 3 || !path[0].is_ascii_alphabetic() || path[1..] != *b":\\" {
        return Err("GetVolumeInformationA supports only guest drive roots".into());
    }
    let drive = format!("{}:", (path[0] as char).to_ascii_lowercase());
    let exists = drive == "c:" || unicorn.get_data().guest_files.directory_exists(&drive);
    // The overlay namespace contains individually mounted assets and transient
    // directories, not a formatted backing volume. No volume serial, label or
    // filesystem capability record is available. Preserve all optional outputs
    // and report the ordinary Win32 failure so callers can handle this absence.
    // A future volume provider must supply actual metadata before returning TRUE.
    unicorn.get_data_mut().windows_last_error = if exists { 50 } else { 15 }; // NOT_SUPPORTED / INVALID_DRIVE
    Ok(0)
}

fn guest_rename(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let old = read_win64_import_argument(unicorn, 0)?;
    let new = read_win64_import_argument(unicorn, 1)?;
    if old == 0 || new == 0 {
        return Err("rename null argument requires invalid parameter handler".into());
    }
    let mut names = Vec::new();
    for address in [old, new] {
        let bytes = read_crt_stdio_c_string(unicorn, address, 1025, "rename path")?;
        if bytes.is_empty() {
            set_guest_crt_errno(unicorn, 2)?;
            return Ok(u32::MAX as u64);
        }
        let absolute = canonical_guest_fullpath(&bytes)?;
        let text = std::str::from_utf8(&absolute[..absolute.len() - 1])
            .map_err(|_| "rename path encoding")?;
        names.push(guest_file_name(text.trim_end_matches(['/', '\\']))?);
    }
    let status = unicorn
        .get_data_mut()
        .guest_files
        .rename_directory(&names[0], &names[1])?;
    if status == 0 {
        Ok(0)
    } else {
        set_guest_crt_errno(unicorn, status)?;
        Ok(u32::MAX as u64)
    }
}

impl GuestFiles {
    fn rename_directory(&mut self, old: &str, new: &str) -> Result<u32, String> {
        if !self.sources.contains_key(old) && !self.directory_exists(old) {
            return Ok(2);
        }
        let prefix = format!("{old}/");
        // The manifest mounts a read-only asset tree. Renaming it must not
        // mutate either the host files or the mount's namespace.
        if self
            .sources
            .keys()
            .any(|path| path == old || path.starts_with(&prefix))
        {
            return Ok(13);
        }
        if self.sources.contains_key(new) || self.directory_exists(new) {
            return Ok(13);
        }
        let (Some((parent, _)), Some((destination_parent, _))) =
            (old.rsplit_once('/'), new.rsplit_once('/'))
        else {
            return Ok(13);
        };
        if parent != destination_parent || !self.directory_exists(destination_parent) {
            return Ok(13);
        }
        if new[2..].contains(['*', '?', ':', '<', '>', '|', '"']) {
            return Ok(13);
        }
        if self.directory_dacls.contains_key(old) || self.directory_dacls.contains_key(parent) {
            return Err("rename directory ACL access evaluation is not implemented".into());
        }
        let affected = self
            .directories
            .iter()
            .filter(|path| path.as_str() == old || path.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        for path in affected {
            let renamed = format!("{new}{}", &path[old.len()..]);
            self.directories.remove(&path);
            self.directories.insert(renamed);
        }
        let affected = self
            .directory_times
            .keys()
            .filter(|path| path.as_str() == old || path.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        for path in affected {
            let times = self.directory_times.remove(&path).unwrap();
            self.directory_times
                .insert(format!("{new}{}", &path[old.len()..]), times);
        }
        let affected = self
            .directory_dacls
            .keys()
            .filter(|path| path.as_str() == old || path.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        for path in affected {
            let acl = self.directory_dacls.remove(&path).unwrap();
            self.directory_dacls
                .insert(format!("{new}{}", &path[old.len()..]), acl);
        }
        let affected = self
            .directory_attributes
            .keys()
            .filter(|path| path.as_str() == old || path.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        for path in affected {
            let attributes = self.directory_attributes.remove(&path).unwrap();
            self.directory_attributes
                .insert(format!("{new}{}", &path[old.len()..]), attributes);
        }
        if let Some(times) = self.directory_times.get_mut(parent) {
            times[1] = std::time::SystemTime::now();
        }
        Ok(0)
    }
}

fn guest_file_attributes_ex(
    unicorn: &mut Unicorn<'_, GuestState>,
    wide: bool,
) -> Result<u64, String> {
    let path = read_win64_import_argument(unicorn, 0)?;
    let level = read_win64_import_argument(unicorn, 1)? as u32;
    let output = read_win64_import_argument(unicorn, 2)?;
    if level != 0 || path == 0 {
        unicorn.get_data_mut().windows_last_error = 87;
        return Ok(0);
    }
    if output == 0 || !guest_range_has_permission(unicorn, output, 36, Prot::WRITE)? {
        unicorn.get_data_mut().windows_last_error = 998;
        return Ok(0);
    }
    let bytes = if wide {
        let mut units = Vec::new();
        for i in 0..=1024u64 {
            let address = path
                .checked_add(i * 2)
                .ok_or("file attributes path overflow")?;
            let raw = acl_read(unicorn, address, 2)?;
            let unit = u16::from_le_bytes([raw[0], raw[1]]);
            if unit == 0 {
                break;
            }
            if i == 1024 {
                return Err("file attributes path exceeds bound".into());
            }
            units.push(unit);
        }
        String::from_utf16(&units)
            .map_err(|_| "file attributes invalid UTF16")?
            .into_bytes()
    } else {
        read_crt_stdio_c_string(unicorn, path, 1025, "file attributes path")?
    };
    if bytes.is_empty() {
        unicorn.get_data_mut().windows_last_error = 3;
        return Ok(0);
    }
    let absolute = canonical_guest_fullpath(&bytes)?;
    let text = std::str::from_utf8(&absolute[..absolute.len() - 1])
        .map_err(|_| "file attributes encoding")?;
    let name = guest_file_name(text.trim_end_matches(['/', '\\']))?;
    let files = &unicorn.get_data().guest_files;
    let mut record = [0u8; 36];
    let times;
    if let Some(source) = files.sources.get(&name) {
        let metadata = match std::fs::metadata(source) {
            Ok(metadata) => metadata,
            Err(error) => {
                unicorn.get_data_mut().windows_last_error = match error.kind() {
                    std::io::ErrorKind::NotFound => 2,
                    std::io::ErrorKind::PermissionDenied => 5,
                    _ => return Err(format!("file attributes metadata: {error}")),
                };
                return Ok(0);
            }
        };
        if !metadata.is_file() {
            return Err("file attributes mounted source is not a file".into());
        }
        record[..4].copy_from_slice(&1u32.to_le_bytes());
        record[28..32].copy_from_slice(&((metadata.len() >> 32) as u32).to_le_bytes());
        record[32..36].copy_from_slice(&(metadata.len() as u32).to_le_bytes());
        times = [metadata.created(), metadata.accessed(), metadata.modified()]
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("file attributes time: {e}"))?;
    } else if files.directory_exists(&name) {
        let attributes = files
            .directory_attributes
            .get(&name)
            .copied()
            .unwrap_or_default();
        record[..4].copy_from_slice(&(0x10u32 | (attributes & !128)).to_le_bytes());
        let stored = files
            .directory_times
            .get(&name)
            .ok_or("file attributes missing directory times")?;
        times = vec![stored[2], stored[0], stored[1]];
    } else {
        let parent_exists = name
            .rsplit_once('/')
            .is_some_and(|(parent, _)| files.directory_exists(parent));
        unicorn.get_data_mut().windows_last_error = if parent_exists { 2 } else { 3 };
        return Ok(0);
    }
    for (i, time) in times.into_iter().enumerate() {
        record[4 + i * 8..12 + i * 8].copy_from_slice(&windows_filetime(time)?.to_le_bytes());
    }
    unicorn
        .mem_write(output, &record)
        .map_err(|e| e.to_string())?;
    Ok(1)
}

fn guest_fwrite(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let input = read_win64_import_argument(unicorn, 0)?;
    let size = read_win64_import_argument(unicorn, 1)?;
    let count = read_win64_import_argument(unicorn, 2)?;
    let token = read_win64_import_argument(unicorn, 3)?;
    if size == 0 || count == 0 {
        return Ok(0);
    }
    let length = size.checked_mul(count).ok_or("fwrite size overflow")?;
    if length > MAX_GUEST_FILE_BYTES as u64 {
        return Err("fwrite request exceeds byte bound".into());
    }
    require_unbuffered_guest_stream(unicorn, token)?;
    let files = &unicorn.get_data().guest_files;
    let stream = files
        .streams
        .get(&token)
        .ok_or("fwrite received stale or foreign FILE")?;
    if stream.readable {
        return Err("fwrite received a read-only FILE".into());
    }
    if !files.standard_streams[1..].contains(&Some(token)) {
        return Err("fwrite output file backend is not implemented".into());
    }
    let bytes = acl_read(unicorn, input, length)?;
    let mut translated = Vec::with_capacity(bytes.len());
    for byte in bytes {
        if byte == b'\n' {
            translated.push(b'\r');
        }
        translated.push(byte);
    }
    if stream.bytes.len().saturating_add(translated.len()) > MAX_GUEST_FILE_BYTES
        || files.live_bytes.saturating_add(translated.len()) > MAX_GUEST_STREAM_BYTES
    {
        return Err("fwrite stream capacity exceeded".into());
    }
    // The worker stdout carries its JSON protocol. Both guest console streams
    // are captured on the worker diagnostic pipe instead, never in that JSON.
    unicorn
        .get_data_mut()
        .guest_files
        .streams
        .get_mut(&token)
        .unwrap()
        .console_repeat
        .write(&translated, &mut std::io::stderr().lock())
        .map_err(|e| format!("guest console write failed: {e}"))?;
    let files = &mut unicorn.get_data_mut().guest_files;
    files.live_bytes += translated.len();
    let stream = files.streams.get_mut(&token).unwrap();
    stream.bytes.extend_from_slice(&translated);
    stream.position = stream.bytes.len();
    Ok(count)
}
