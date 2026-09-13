// Session-owned Windows file handles for explicitly mounted read-only assets.
// Separate token space from CRT FILE pointers; issued handles are never reused.
const WINDOWS_ASSET_HANDLE_BASE: u64 = 0xc00000000;
struct WindowsAssetFile {
    name: String,
    bytes: Box<[u8]>,
    position: u64,
    readable: bool,
    share_read_access: bool,
    share: u32,
}

impl GuestFiles {
    fn open_windows_asset(
        &mut self,
        name: &str,
        readable: bool,
        share_read_access: bool,
        share: u32,
    ) -> Result<Result<u64, u32>, String> {
        use std::io::Read;
        if share & !7 != 0 {
            return Ok(Err(87));
        }
        if self.directory_exists(name) {
            return Ok(Err(5));
        }
        let Some(source) = self.sources.get(name) else {
            return Ok(Err(2));
        };
        if self.windows_files.values().any(|file| {
            file.name == name
                && ((share_read_access && file.share & 1 == 0)
                    || (file.share_read_access && share & 1 == 0))
        }) || (share & 1 == 0
            && self
                .streams
                .values()
                .any(|file| file.name.as_deref() == Some(name) && file.readable))
        {
            return Ok(Err(32));
        }
        if self.windows_files.len() + self.streams.len() >= 64 || self.next_windows_file >= 65536 {
            return Ok(Err(4));
        }
        let file = match std::fs::File::open(source) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Err(2)),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(Err(5)),
            Err(e) => return Err(format!("Windows asset open: {e}")),
        };
        if !file
            .metadata()
            .map_err(|e| format!("Windows asset metadata: {e}"))?
            .is_file()
        {
            return Ok(Err(5));
        }
        let capacity = MAX_GUEST_FILE_BYTES.min(
            MAX_GUEST_STREAM_BYTES
                .checked_sub(self.live_bytes)
                .ok_or("guest asset byte accounting overflow")?,
        );
        let mut bytes = Vec::new();
        file.take(capacity as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Windows asset read: {e}"))?;
        if bytes.len() > capacity {
            return Err("Windows asset byte capacity exceeded".into());
        }
        let sha = format!("{:x}", Sha256::digest(&bytes));
        let handle = WINDOWS_ASSET_HANDLE_BASE + self.next_windows_file * 8;
        self.next_windows_file += 1;
        self.live_bytes += bytes.len();
        self.windows_files.insert(
            handle,
            WindowsAssetFile {
                name: name.into(),
                bytes: bytes.into_boxed_slice(),
                position: 0,
                readable,
                share_read_access,
                share,
            },
        );
        self.reports.push(TraceModule {
            name: name.into(),
            kind: "guest_asset",
            sha256: Some(sha),
            symbols: vec![],
        });
        Ok(Ok(handle))
    }

    fn close_windows_asset(&mut self, handle: u64) -> Result<(), u32> {
        let Some(file) = self.windows_files.remove(&handle) else {
            return Err(6);
        };
        self.live_bytes -= file.bytes.len();
        Ok(())
    }
}
