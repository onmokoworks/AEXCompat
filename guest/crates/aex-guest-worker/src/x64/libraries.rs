// Keep dependency images outside all lazy heap, PF/world, environment, and
// thread-stack arenas, even when those regions have not been mapped yet.
const DEPENDENCY_IMAGE_BASE: u64 = 0x0000_0018_0000_0000;
const DEPENDENCY_IMAGE_END: u64 = ENVIRONMENT_STRINGS_BASE;
const SAPPHIRE_LARGE_STRING_ASSIGN_RVA: u64 = 0x0c_7840;
const SAPPHIRE_LUT_COPY_SITE_RVA: u64 = 0x0874_81c;
const SAPPHIRE_LUT_COPY_CONTINUE_RVA: u64 = 0x0874_8e1;
const SAPPHIRE_LUT_COPY_SITE_BYTES: &[u8] = &[
    0x49, 0x29, 0xd0, 0x48, 0x8d, 0x8d, 0xe0, 0x09, 0x00, 0x00, 0x48, 0x89, 0xce, 0xe8,
];
const SAPPHIRE_LARGE_STRING_ASSIGN_PROLOGUE: &[u8] = &[
    0x56, 0x57, 0x53, 0x48, 0x83, 0xec, 0x40, 0x4c, 0x89, 0xc7, 0x48, 0x89, 0xce, 0x48, 0x8b,
    0x41, 0x18,
];
const SAPPHIRE_LARGE_STRING_MIN_BYTES: u64 = 1024 * 1024;
const SAPPHIRE_FILEBUF_FGETC_CALL_RVA: u64 = 0x05cc_caf;
const SAPPHIRE_FILEBUF_FGETC_RETURN_RVA: u64 = 0x05cc_cb5;
const SAPPHIRE_FILEBUF_FGETC_CALL_BYTES: &[u8] = &[0xff, 0x15, 0x8b, 0x29, 0xc9, 0x02];

fn install_sapphire_large_string_assign(
    unicorn: &mut Unicorn<'static, GuestState>,
) -> Result<(), GuestError> {
    let Some(library) = guest_library_by_name(unicorn.get_data(), "sapphire_ae.dll") else {
        return Ok(());
    };
    let library_base = library.base;
    let address = library_base
        .checked_add(SAPPHIRE_LARGE_STRING_ASSIGN_RVA)
        .ok_or(GuestError::ImageAlignment)?;
    let mut actual = vec![0; SAPPHIRE_LARGE_STRING_ASSIGN_PROLOGUE.len()];
    uc(
        "read Sapphire large string assign prologue",
        unicorn.mem_read(address, &mut actual),
    )?;
    if actual != SAPPHIRE_LARGE_STRING_ASSIGN_PROLOGUE {
        return Ok(());
    }
    let mut fgetc_call = vec![0; SAPPHIRE_FILEBUF_FGETC_CALL_BYTES.len()];
    uc(
        "read Sapphire filebuf fgetc call",
        unicorn.mem_read(
            library_base + SAPPHIRE_FILEBUF_FGETC_CALL_RVA,
            &mut fgetc_call,
        ),
    )?;
    if fgetc_call == SAPPHIRE_FILEBUF_FGETC_CALL_BYTES {
        unicorn.get_data_mut().sapphire_filebuf_fgetc_return =
            Some(library_base + SAPPHIRE_FILEBUF_FGETC_RETURN_RVA);
    }
    let copy_site = library_base + SAPPHIRE_LUT_COPY_SITE_RVA;
    let mut copy_site_actual = vec![0; SAPPHIRE_LUT_COPY_SITE_BYTES.len()];
    uc(
        "read Sapphire LUT copy site",
        unicorn.mem_read(copy_site, &mut copy_site_actual),
    )?;
    if copy_site_actual == SAPPHIRE_LUT_COPY_SITE_BYTES {
        let continuation = library_base + SAPPHIRE_LUT_COPY_CONTINUE_RVA;
        uc(
            "install Sapphire LUT copy fast path",
            unicorn.add_code_hook(copy_site, copy_site, move |unicorn, _, _| {
                if let Err(error) = emulate_sapphire_lut_copy(unicorn, continuation) {
                    if unicorn.get_data().callback_error.is_none() {
                        unicorn.get_data_mut().callback_error = Some(error);
                    }
                    let _ = unicorn.emu_stop();
                }
            }),
        )?;
    }
    uc(
        "install Sapphire large string assign",
        unicorn.add_code_hook(address, address, |unicorn, _, _| {
            if let Err(error) = emulate_sapphire_large_string_assign(unicorn) {
                if unicorn.get_data().callback_error.is_none() {
                    unicorn.get_data_mut().callback_error = Some(error);
                }
                let _ = unicorn.emu_stop();
            }
        }),
    )
    .map(|_| ())
}

fn emulate_sapphire_lut_copy(
    unicorn: &mut Unicorn<'_, GuestState>,
    continuation: u64,
) -> Result<(), String> {
    let source = unicorn
        .reg_read(RegisterX86::RDX)
        .map_err(|error| format!("read Sapphire LUT source: {error}"))?;
    let end = unicorn
        .reg_read(RegisterX86::R8)
        .map_err(|error| format!("read Sapphire LUT end: {error}"))?;
    let Some(length) = end.checked_sub(source) else {
        return Ok(());
    };
    if source == 0
        || length < SAPPHIRE_LARGE_STRING_MIN_BYTES
        || length > MAX_CRT_ALLOCATION_BYTES
    {
        return Ok(());
    }
    let output = allocate_crt_region(unicorn, length + 1).map_err(|error| error.to_string())?;
    let mut offset = 0u64;
    let mut chunk = vec![0u8; 1024 * 1024];
    while offset < length {
        let count = usize::try_from((length - offset).min(chunk.len() as u64)).unwrap();
        unicorn
            .mem_read(source + offset, &mut chunk[..count])
            .map_err(|error| format!("read Sapphire LUT bytes: {error}"))?;
        unicorn
            .mem_write(output + offset, &chunk[..count])
            .map_err(|error| format!("write Sapphire LUT bytes: {error}"))?;
        offset += count as u64;
    }
    unicorn
        .mem_write(output + length, &[0])
        .map_err(|error| format!("terminate Sapphire LUT: {error}"))?;
    let rbp = unicorn
        .reg_read(RegisterX86::RBP)
        .map_err(|error| format!("read Sapphire LUT frame: {error}"))?;
    let mut owner_bytes = [0u8; 8];
    unicorn
        .mem_read(rbp + 0x8c0, &mut owner_bytes)
        .map_err(|error| format!("read Sapphire LUT owner: {error}"))?;
    let owner = u64::from_le_bytes(owner_bytes);
    unicorn
        .mem_write(owner + 0x108, &output.to_le_bytes())
        .map_err(|error| format!("write Sapphire LUT pointer: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RIP, continuation)
        .map_err(|error| format!("continue Sapphire LUT setup: {error}"))?;
    Ok(())
}

fn emulate_sapphire_large_string_assign(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), String> {
    let object = unicorn
        .reg_read(RegisterX86::RCX)
        .map_err(|error| format!("read Sapphire string object: {error}"))?;
    let source = unicorn
        .reg_read(RegisterX86::RDX)
        .map_err(|error| format!("read Sapphire string source: {error}"))?;
    let length = unicorn
        .reg_read(RegisterX86::R8)
        .map_err(|error| format!("read Sapphire string length: {error}"))?;
    let mut metadata = [0u8; 16];
    if object == 0
        || source == 0
        || length < SAPPHIRE_LARGE_STRING_MIN_BYTES
        || length > MAX_CRT_ALLOCATION_BYTES
        || unicorn.mem_read(object + 16, &mut metadata).is_err()
        || u64::from_le_bytes(metadata[..8].try_into().unwrap()) != 0
        || u64::from_le_bytes(metadata[8..].try_into().unwrap()) != 15
    {
        return Ok(());
    }

    let capacity = length | 15;
    let allocation_size = capacity
        .checked_add(40)
        .ok_or_else(|| "Sapphire string allocation overflow".to_string())?;
    let base = allocate_crt_region(unicorn, allocation_size).map_err(|error| error.to_string())?;
    let data = base
        .checked_add(39)
        .map(|address| address & !31)
        .ok_or_else(|| "Sapphire aligned string pointer overflow".to_string())?;
    unicorn
        .mem_write(data - 8, &base.to_le_bytes())
        .map_err(|error| format!("write Sapphire string allocation base: {error}"))?;
    let mut offset = 0u64;
    let mut chunk = vec![0u8; 1024 * 1024];
    while offset < length {
        let count = usize::try_from((length - offset).min(chunk.len() as u64)).unwrap();
        unicorn
            .mem_read(source + offset, &mut chunk[..count])
            .map_err(|error| format!("read Sapphire string bytes: {error}"))?;
        unicorn
            .mem_write(data + offset, &chunk[..count])
            .map_err(|error| format!("write Sapphire string bytes: {error}"))?;
        offset += count as u64;
    }
    unicorn
        .mem_write(data + length, &[0])
        .map_err(|error| format!("terminate Sapphire string: {error}"))?;
    unicorn
        .mem_write(object, &data.to_le_bytes())
        .map_err(|error| format!("write Sapphire string pointer: {error}"))?;
    unicorn
        .mem_write(object + 16, &length.to_le_bytes())
        .map_err(|error| format!("write Sapphire string length: {error}"))?;
    unicorn
        .mem_write(object + 24, &capacity.to_le_bytes())
        .map_err(|error| format!("write Sapphire string capacity: {error}"))?;
    let rsp = unicorn
        .reg_read(RegisterX86::RSP)
        .map_err(|error| format!("read Sapphire string stack: {error}"))?;
    let mut return_bytes = [0u8; 8];
    unicorn
        .mem_read(rsp, &mut return_bytes)
        .map_err(|error| format!("read Sapphire string return address: {error}"))?;
    let return_address = u64::from_le_bytes(return_bytes);
    unicorn
        .reg_write(RegisterX86::RAX, object)
        .map_err(|error| format!("write Sapphire string result: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RSP, rsp + 8)
        .map_err(|error| format!("restore Sapphire string stack: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RIP, return_address)
        .map_err(|error| format!("return from Sapphire string assign: {error}"))?;
    Ok(())
}

/// A real mapped DLL, pinned for the lifetime of the explicit library set.
#[derive(Clone)]
struct GuestLibrary {
    base: u64,
    end: u64,
    exports: BTreeMap<String, u64>,
    ordinal_exports: BTreeMap<u32, u64>,
    report: crate::pe::PeReport,
    initialized: bool,
}

fn normalized_library_name(name: &str) -> Result<String, GuestError> {
    if name.is_empty() || name.len() > 1024 || name.bytes().any(|b| b == 0 || !b.is_ascii()) {
        return Err(GuestError::Callback("invalid explicit DLL name".into()));
    }
    let name = name.replace('\\', "/").to_ascii_lowercase();
    if name
        .split('/')
        .any(|part| part == "." || part == ".." || part.is_empty())
    {
        return Err(GuestError::Callback("ambiguous explicit DLL path".into()));
    }
    Ok(name)
}

fn dependency_parse_batch_len(pending: usize, maximum_workers: usize) -> usize {
    if pending == 0 {
        return 0;
    }
    maximum_workers.min(pending)
}

fn prefer_emulated_dependency_import(library: &str, symbol: &str) -> bool {
    library.eq_ignore_ascii_case("msvcp140.dll")
        && matches!(
            symbol,
            "?in@?$codecvt@_WDU_Mbstatet@@@std@@QEBAHAEAU_Mbstatet@@PEBD1AEAPEBDPEA_W3AEAPEA_W@Z"
                | "?out@?$codecvt@_WDU_Mbstatet@@@std@@QEBAHAEAU_Mbstatet@@PEB_W1AEAPEB_WPEAD3AEAPEAD@Z"
        )
}

fn guest_module_from_address(state: &GuestState, address: u64) -> Option<u64> {
    state
        .image_region
        .filter(|(start, end)| (*start..*end).contains(&address))
        .map(|(start, _)| start)
        .or_else(|| {
            state
                .loaded_libraries
                .values()
                .find(|library| (library.base..library.end).contains(&address))
                .map(|library| library.base)
        })
}

fn guest_library_by_name<'a>(state: &'a GuestState, name: &str) -> Option<&'a GuestLibrary> {
    let name = name.replace('\\', "/").to_ascii_lowercase();
    state.loaded_libraries.get(&name).or_else(|| {
        if name.contains('/') {
            return None;
        }
        state
            .loaded_libraries
            .iter()
            .find(|(path, _)| path.rsplit('/').next() == Some(name.as_str()))
            .map(|(_, library)| library)
    })
}

impl GuestEngine<'static> {
    /// Map explicit dependency images, link their IATs, and run process attach
    /// in dependency order before attaching the primary image. Images must have
    /// already been rebased into distinct guest regions. No native DLL is loaded.
    pub fn load_with_libraries(
        image: &PeImage,
        libraries: &[(&str, PeImage)],
    ) -> Result<Self, GuestError> {
        Self::load_with_libraries_inner(image, libraries, true)
    }

    /// Build a process template with every explicit dependency initialized,
    /// while leaving the primary image before process attach. A forkserver can
    /// copy this quiescent state and attach one primary image in each child.
    pub fn load_with_libraries_deferred_primary(
        image: &PeImage,
        libraries: &[(&str, PeImage)],
    ) -> Result<Self, GuestError> {
        Self::load_with_libraries_inner(image, libraries, false)
    }

    fn load_with_libraries_inner(
        image: &PeImage,
        libraries: &[(&str, PeImage)],
        attach_primary: bool,
    ) -> Result<Self, GuestError> {
        if libraries.is_empty() {
            return Self::load_primary(image, attach_primary);
        }
        if libraries.len() > 64 {
            return Err(GuestError::Callback("DLL count exceeds 64".into()));
        }
        let mut total_bytes = image.mapped_bytes().len();
        let mut names = BTreeSet::new();
        let mut basenames = BTreeMap::new();
        for (index, (name, library)) in libraries.iter().enumerate() {
            let normalized = normalized_library_name(name)?;
            if !names.insert(normalized.clone()) {
                return Err(GuestError::Callback("duplicate DLL path".into()));
            }
            let basename = normalized.rsplit('/').next().unwrap().to_string();
            if basenames.insert(basename, index).is_some() {
                return Err(GuestError::Callback("ambiguous DLL basename".into()));
            }
            total_bytes = total_bytes
                .checked_add(library.mapped_bytes().len())
                .ok_or(GuestError::DataCapacity)?;
            if total_bytes > 1024 * 1024 * 1024 {
                return Err(GuestError::Callback("mapped DLL set exceeds 1 GiB".into()));
            }
        }
        // Cycle rejection is explicit until loader-lock/partial-initialization
        // semantics for mutually importing DLLs are implemented.
        fn visit(
            index: usize,
            libraries: &[(&str, PeImage)],
            names: &BTreeMap<String, usize>,
            visiting: &mut [u8],
            order: &mut Vec<usize>,
        ) -> Result<(), GuestError> {
            if visiting[index] == 2 {
                return Ok(());
            }
            if visiting[index] == 1 {
                return Err(GuestError::Callback(
                    "cyclic DLL initialization dependency".into(),
                ));
            }
            visiting[index] = 1;
            for import in libraries[index].1.imports() {
                if let Some(dependency) = names.get(&import.name.to_ascii_lowercase()) {
                    visit(*dependency, libraries, names, visiting, order)?;
                }
            }
            visiting[index] = 2;
            order.push(index);
            Ok(())
        }
        let mut order = Vec::new();
        let mut visiting = vec![0; libraries.len()];
        for index in 0..libraries.len() {
            visit(index, libraries, &basenames, &mut visiting, &mut order)?;
        }
        let mut engine = Self::load_primary(image, false)?;
        for (name, library) in libraries {
            let size = library.mapped_bytes().len() as u64;
            let base = library.image_base();
            let end = base.checked_add(size).ok_or(GuestError::ImageAlignment)?;
            if base < DEPENDENCY_IMAGE_BASE || end > DEPENDENCY_IMAGE_END {
                return Err(GuestError::Callback(
                    "DLL overlaps reserved guest address space".into(),
                ));
            }
            if base % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
                return Err(GuestError::ImageAlignment);
            }
            uc(
                "map dependency DLL",
                engine.unicorn.mem_map(base, size, Prot::ALL),
            )?;
            uc(
                "write dependency DLL",
                engine.unicorn.mem_write(base, library.mapped_bytes()),
            )?;
            for section in library
                .section_protections()
                .iter()
                .filter(|s| s.executable)
            {
                let start = base
                    .checked_add(section.virtual_address as u64)
                    .ok_or(GuestError::ImageAlignment)?;
                let stop = start
                    .checked_add(section.virtual_size as u64)
                    .filter(|stop| *stop <= end)
                    .ok_or(GuestError::ImageAlignment)?;
                if start < stop {
                    let section_start = section.virtual_address;
                    let section_end = section_start
                        .checked_add(section.virtual_size)
                        .filter(|end| *end <= library.mapped_bytes().len())
                        .ok_or(GuestError::ImageAlignment)?;
                    let points = discover_avx_state_sync_points_with_limit(
                        &library.mapped_bytes()[section_start..section_end],
                        start,
                        MAX_RUNTIME_AVX_STATE_SYNC_POINTS,
                    )?;
                    install_runtime_avx_state_sync(&mut engine.unicorn, points)?;
                    engine
                        .unicorn
                        .get_data_mut()
                        .image_executable_ranges
                        .push((start, stop));
                }
            }
            let exports = library
                .exports()
                .keys()
                .filter_map(|name| {
                    library
                        .symbol_address(name)
                        .map(|address| (name.clone(), address))
                })
                .collect();
            let ordinal_exports = library
                .ordinal_exports()
                .keys()
                .filter_map(|ordinal| {
                    library
                        .ordinal_address(*ordinal)
                        .map(|address| (*ordinal, address))
                })
                .collect();
            engine.trace_modules.push(TraceModule {
                name: name.rsplit(['/', '\\']).next().unwrap().to_string(),
                kind: "mapped_dependency_pe",
                sha256: Some(library.report().sha256.clone()),
                symbols: library.exports().keys().cloned().collect(),
            });
            engine.unicorn.get_data_mut().loaded_libraries.insert(
                normalized_library_name(name)?,
                GuestLibrary {
                    base,
                    end,
                    exports,
                    ordinal_exports,
                    report: library.report(),
                    initialized: false,
                },
            );
        }
        install_translated_avx_state_sync(&mut engine.unicorn)?;
        // Re-link primary imports as well: a named dependency must resolve to
        // its real code/data export, never an unrelated emulation stub.
        for target in std::iter::once(image).chain(libraries.iter().map(|(_, image)| image)) {
            for import in target.imports() {
                for symbol in &import.symbols {
                    let address =
                        if let Some(index) = basenames.get(&import.name.to_ascii_lowercase())
                            && !prefer_emulated_dependency_import(&import.name, &symbol.name)
                        {
                            symbol
                                .ordinal
                                .and_then(|ordinal| libraries[*index].1.ordinal_address(ordinal.into()))
                                .or_else(|| libraries[*index].1.symbol_address(&symbol.name))
                                .ok_or_else(|| {
                                    GuestError::Callback(format!(
                                        "DLL export unavailable: {}!{}",
                                        import.name, symbol.name
                                    ))
                                })?
                        } else if let Some(data) =
                            engine.resolve_emulated_import_data(&import.name, &symbol.name)?
                        {
                            data
                        } else if std::ptr::eq(target, image) {
                            continue; // primary's emulated imports already installed
                        } else {
                            let stub = STUB_BASE
                                .checked_add(engine.next_import_stub * STUB_STRIDE)
                                .ok_or(GuestError::StubCapacity)?;
                            if stub + STUB_STRIDE > HOST_ADD_PARAM {
                                return Err(GuestError::StubCapacity);
                            }
                            uc(
                                "write DLL import stub",
                                engine.unicorn.mem_write(stub, &[0xc3]),
                            )?;
                            install_win64_import(
                                &mut engine.unicorn,
                                stub,
                                &import.name,
                                &symbol.name,
                            )?;
                            engine.unicorn.get_data_mut().trace_labels.insert(
                                stub,
                                TraceLabel {
                                    kind: TraceLabelKind::Import,
                                    name: canonical_import_trace_label(&import.name, &symbol.name),
                                },
                            );
                            engine.next_import_stub += 1;
                            stub
                        };
                    let rva = symbol.iat_rva;
                    if rva
                        .checked_add(8)
                        .is_none_or(|end| end > target.mapped_bytes().len())
                    {
                        return Err(GuestError::IatRange);
                    }
                    uc(
                        "link DLL IAT",
                        engine
                            .unicorn
                            .mem_write(target.image_base() + rva as u64, &address.to_le_bytes()),
                    )?;
                }
            }
        }
        // Each module receives a distinct TLS index, template block and array
        // slot. This supersedes primary's initial one-slot array before attach.
        let tls_images: Vec<_> = std::iter::once(image)
            .chain(libraries.iter().map(|(_, image)| image))
            .filter_map(|image| image.static_tls())
            .collect();
        if !tls_images.is_empty() {
            let array = engine.allocate(tls_images.len() * 8, 8)?;
            for (index, tls) in tls_images.iter().enumerate() {
                let block = engine.allocate(tls.bytes.len().max(1), 16)?;
                engine.write(block, &tls.bytes)?;
                engine.write(array + index as u64 * 8, &block.to_le_bytes())?;
                engine.write(tls.index_address, &(index as u32).to_le_bytes())?;
            }
            engine.write(0x58, &array.to_le_bytes())?;
        }
        install_sapphire_large_string_assign(&mut engine.unicorn)?;
        for (_, library) in libraries {
            seal_unicorn_image(
                &mut engine.unicorn,
                library,
                library.mapped_bytes().len() as u64,
            )?;
        }
        for index in order {
            let (name, library) = &libraries[index];
            engine
                .run_process_attach_addresses(
                    library.image_base(),
                    library.tls_callbacks(),
                    library.dll_entry_address(),
                )
                .map_err(|error| {
                    GuestError::Callback(format!(
                        "DLL initialization {} sha256={}: {error}",
                        name.rsplit('/').next().unwrap_or(name),
                        library.report().sha256
                    ))
                })?;
            engine
                .unicorn
                .get_data_mut()
                .loaded_libraries
                .get_mut(&normalized_library_name(name)?)
                .unwrap()
                .initialized = true;
        }
        if attach_primary {
            engine.run_process_attach_addresses(
                image.image_base(),
                image.tls_callbacks(),
                image.dll_entry_address(),
            )?;
            engine.primary_attached = true;
        }
        Ok(engine)
    }

    /// Complete process attach for the primary image of a deferred template.
    pub fn attach_deferred_primary(&mut self, image: &PeImage) -> Result<(), GuestError> {
        if self.primary_attached
            || self.primary_poisoned
            || image.report().sha256 != self.image_sha256
            || image.image_base() != self.image_base
            || image.image_base().checked_add(image.mapped_bytes().len() as u64)
                != Some(self.image_end)
        {
            return Err(GuestError::Callback(
                "deferred primary image does not match the prepared mapping".into(),
            ));
        }
        if let Err(error) = self.run_process_attach_addresses(
            image.image_base(),
            image.tls_callbacks(),
            image.dll_entry_address(),
        ) {
            self.primary_poisoned = true;
            return Err(error);
        }
        self.unicorn.get_data_mut().sealed_image_reads = true;
        self.primary_attached = true;
        Ok(())
    }

    pub fn validate_attached_primary(&self, image: &PeImage) -> Result<(), GuestError> {
        if !self.primary_attached
            || self.primary_poisoned
            || image.report().sha256 != self.image_sha256
            || image.image_base() != self.image_base
            || image.image_base().checked_add(image.mapped_bytes().len() as u64)
                != Some(self.image_end)
        {
            return Err(GuestError::Callback(
                "Classic host primary does not match the attached guest image".into(),
            ));
        }
        Ok(())
    }

    /// Explicit local manifest for the Unicorn backend. This records actual
    /// bytes in library reports; hashes never gate permission to load.
    pub fn load_with_library_manifest(
        image: &PeImage,
        path: &std::path::Path,
    ) -> Result<Self, GuestError> {
        Self::load_with_library_manifest_inner(image, path, true)
    }

    pub fn load_with_library_manifest_deferred_primary(
        image: &PeImage,
        path: &std::path::Path,
    ) -> Result<Self, GuestError> {
        Self::load_with_library_manifest_inner(image, path, false)
    }

    fn load_with_library_manifest_inner(
        image: &PeImage,
        path: &std::path::Path,
        attach_primary: bool,
    ) -> Result<Self, GuestError> {
        use std::io::Read;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Manifest {
            libraries: Vec<Entry>,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Entry {
            name: String,
            path: std::path::PathBuf,
        }
        fn read_bounded(path: &std::path::Path, maximum: usize) -> Result<Vec<u8>, GuestError> {
            let file = std::fs::File::open(path)
                .map_err(|e| GuestError::Callback(format!("open DLL manifest/input: {e}")))?;
            let mut bytes = Vec::new();
            file.take(maximum as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| GuestError::Callback(format!("read DLL manifest/input: {e}")))?;
            if bytes.len() > maximum {
                return Err(GuestError::Callback(
                    "DLL manifest/input exceeds byte bound".into(),
                ));
            }
            Ok(bytes)
        }
        fn parse_dependency(
            name: &str,
            path: &std::path::Path,
        ) -> Result<PeImage, GuestError> {
            let bytes = read_bounded(path, 128 * 1024 * 1024).map_err(|error| {
                GuestError::Callback(format!("read dependency DLL {name}: {error}"))
            })?;
            PeImage::parse_library(&bytes).map_err(|error| {
                GuestError::Callback(format!("parse dependency DLL {name}: {error}"))
            })
        }
        let manifest: Manifest = serde_json::from_slice(&read_bounded(path, 1024 * 1024)?)
            .map_err(|e| GuestError::Callback(format!("parse DLL manifest: {e}")))?;
        if manifest.libraries.len() > 64 {
            return Err(GuestError::Callback("DLL count exceeds 64".into()));
        }
        let manifest_directory = path.parent().unwrap_or(std::path::Path::new("."));
        let mut inputs = Vec::with_capacity(manifest.libraries.len());
        for (index, entry) in manifest.libraries.into_iter().enumerate() {
            normalized_library_name(&entry.name)?;
            let file_path = if entry.path.is_absolute() {
                entry.path
            } else {
                manifest_directory.join(entry.path)
            };
            inputs.push((index, entry.name, file_path));
        }

        // Reading and parsing dependency images is independent work. Parse at
        // most four at once and charge each completed batch to the retained
        // 1 GiB set before starting another. With the 256 MiB per-image parser
        // bound, transient mapped-image memory is therefore bounded below
        // 2 GiB instead of retaining all 64 manifest entries (up to 16 GiB).
        let maximum_workers = inputs
            .len()
            .min(
                std::thread::available_parallelism()
                    .map(std::num::NonZeroUsize::get)
                    .unwrap_or(1),
            )
            .min(4);
        const MAX_MAPPED_SET_BYTES: usize = 1024 * 1024 * 1024;
        let mut libraries = Vec::with_capacity(inputs.len());
        let mut base = DEPENDENCY_IMAGE_BASE;
        let mut total = image.mapped_bytes().len();
        let mut offset = 0;
        while offset < inputs.len() {
            let batch_len = dependency_parse_batch_len(inputs.len() - offset, maximum_workers);
            let batch = &inputs[offset..offset + batch_len];
            let parsed = if batch_len == 1 {
                let (index, name, file_path) = &batch[0];
                vec![(*index, name.clone(), parse_dependency(name, file_path))]
            } else {
                std::thread::scope(|scope| {
                    let handles = batch
                        .iter()
                        .map(|(index, name, file_path)| {
                            scope.spawn(move || {
                                (*index, name.clone(), parse_dependency(name, file_path))
                            })
                        })
                        .collect::<Vec<_>>();
                    handles
                        .into_iter()
                        .map(|handle| {
                            handle.join().map_err(|_| {
                                GuestError::Callback(
                                    "dependency DLL parser thread panicked".into(),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })?
            };
            for (_, name, library) in parsed {
                let library = library?.rebase(base).map_err(|error| {
                    GuestError::Callback(format!("parse dependency DLL {name}: {error}"))
                })?;
                total = total
                    .checked_add(library.mapped_bytes().len())
                    .ok_or(GuestError::DataCapacity)?;
                if total > MAX_MAPPED_SET_BYTES {
                    return Err(GuestError::Callback("mapped DLL set exceeds 1 GiB".into()));
                }
                base = base
                    .checked_add(library.mapped_bytes().len() as u64 + 65535)
                    .ok_or(GuestError::ImageAlignment)?
                    & !65535;
                libraries.push((name, library));
            }
            offset += batch_len;
        }
        let borrowed: Vec<_> = libraries
            .into_iter()
            .map(|(name, image)| (name, image))
            .collect();
        // Keep names owned while passing the public explicit-image API.
        let names: Vec<_> = borrowed.iter().map(|(name, _)| name.clone()).collect();
        let images: Vec<_> = borrowed
            .into_iter()
            .zip(names.iter())
            .map(|((_, image), name)| (name.as_str(), image))
            .collect();
        Self::load_with_libraries_inner(image, &images, attach_primary)
    }

    pub fn library_reports(&self) -> Vec<crate::pe::PeReport> {
        self.unicorn
            .get_data()
            .loaded_libraries
            .values()
            .map(|library| library.report.clone())
            .collect()
    }
}

#[cfg(test)]
mod library_tests {
    use super::*;

    fn fixture(base: u64, symbol: &str, import: Option<(&str, &str)>, attach: bool) -> PeImage {
        let file = fixture_bytes(base, symbol, import, attach);
        if symbol == "EffectMain" {
            PeImage::parse_and_map(&file).unwrap()
        } else {
            PeImage::parse_library(&file).unwrap()
        }
    }

    #[test]
    fn dll_avx_fallback_executes_real_export_with_wrapping_negative_index() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let base = DEPENDENCY_IMAGE_BASE;
        let mut bytes = fixture_bytes(base, "vector_copy", None, false);
        let code = [
            0x48, 0x89, 0xc8, 0xc4, 0xc1, 0x7c, 0x10, 0x04, 0x00, 0xc5, 0xfc, 0x11, 0x02, 0xc3,
        ];
        bytes[0x200..0x200 + code.len()].copy_from_slice(&code);
        let dll = PeImage::parse_library(&bytes).unwrap();
        let mut engine =
            GuestEngine::load_with_libraries(&primary, &[("vector.dll", dll)]).unwrap();
        let source = engine.allocate(32, 8).unwrap();
        let destination = engine.allocate(32, 8).unwrap();
        let expected = std::array::from_fn::<_, 32, _>(|i| i as u8 ^ 0xa5);
        engine.write(source, &expected).unwrap();
        engine
            .call_win64(
                base + 0x1000,
                [source + 8192, destination, (-8192i64) as u64, 0, 0, 0],
            )
            .unwrap();
        let mut actual = [0; 32];
        engine.read(destination, &mut actual).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 2);
    }

    fn fixture_bytes(
        base: u64,
        symbol: &str,
        import: Option<(&str, &str)>,
        attach: bool,
    ) -> Vec<u8> {
        let mut file = vec![0u8; 0x800];
        fn p16(file: &mut [u8], at: usize, value: u16) {
            file[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn p32(file: &mut [u8], at: usize, value: u32) {
            file[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        fn p64(file: &mut [u8], at: usize, value: u64) {
            file[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        file[..2].copy_from_slice(b"MZ");
        p32(&mut file, 60, 0x80);
        file[0x80..0x84].copy_from_slice(b"PE\0\0");
        p16(&mut file, 0x84, 0x8664);
        p16(&mut file, 0x86, 2);
        p16(&mut file, 0x94, 240);
        p16(&mut file, 0x96, 0x2022);
        let op = 0x98;
        p16(&mut file, op, 0x20b);
        p64(&mut file, op + 24, base);
        for (at, value) in [
            (32, 4096),
            (36, 512),
            (56, 0x3000),
            (60, 0x200),
            (108, 16),
            (112, 0x2000),
            (116, 0x100),
        ] {
            p32(&mut file, op + at, value);
        }
        for (index, name, va, raw, size, flags) in [
            (0, b".text", 0x1000, 0x200, 0x200, 0x60000020),
            (1, b".data", 0x2000, 0x400, 0x400, 0xc0000040),
        ] {
            let section = op + 240 + index * 40;
            file[section..section + 5].copy_from_slice(name);
            for (at, value) in [(8, size), (12, va), (16, size), (20, raw), (36, flags)] {
                p32(&mut file, section + at, value);
            }
        }
        for (at, value) in [
            (12, 0x2090),
            (16, 1),
            (20, 1),
            (24, 1),
            (28, 0x2040),
            (32, 0x2048),
            (36, 0x2050),
        ] {
            p32(&mut file, 0x400 + at, value);
        }
        p32(&mut file, 0x440, 0x1000);
        p32(&mut file, 0x448, 0x2060);
        file[0x460..0x460 + symbol.len()].copy_from_slice(symbol.as_bytes());
        file[0x490..0x499].copy_from_slice(b"test.dll\0");
        // Export reads the value which DllMain initializes to 42.
        file[0x200..0x207].copy_from_slice(&[0x8b, 0x05, 0xfa, 0x12, 0, 0, 0xc3]);
        if let Some((dll, imported)) = import {
            p32(&mut file, op + 120, 0x2100);
            p32(&mut file, op + 124, 40);
            p32(&mut file, 0x500, 0x2150);
            p32(&mut file, 0x50c, 0x2170);
            p32(&mut file, 0x510, 0x2180);
            p64(&mut file, 0x550, 0x21a0);
            p64(&mut file, 0x580, 0x21a0);
            file[0x570..0x570 + dll.len()].copy_from_slice(dll.as_bytes());
            file[0x5a2..0x5a2 + imported.len()].copy_from_slice(imported.as_bytes());
            // The primary initializer calls the dependency and saves its result.
            file[0x240..0x25a].copy_from_slice(&[
                0x48, 0x83, 0xec, 0x28, 0xff, 0x15, 0x36, 0x11, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0x89,
                0x05, 0xac, 0x12, 0, 0, 0xb8, 1, 0, 0, 0, 0xc3,
            ]);
        } else {
            file[0x240..0x250].copy_from_slice(&[
                0xc7, 0x05, 0xb6, 0x12, 0, 0, 42, 0, 0, 0, 0xb8, 1, 0, 0, 0, 0xc3,
            ]);
        }
        if attach {
            p32(&mut file, op + 16, 0x1040);
        }
        file
    }

    #[test]
    fn links_real_dependency_code_and_initializes_it_before_primary() {
        let primary = fixture(0x180000000, "EffectMain", Some(("dep.dll", "answer")), true);
        let dependency = fixture(0x1800000000, "answer", None, true);
        let expected_sha = dependency.report().sha256;
        let mut engine =
            GuestEngine::load_with_libraries(&primary, &[("C:/runtime/dep.dll", dependency)])
                .unwrap();
        assert_eq!(
            engine
                .call_win64(primary.entry_address().unwrap(), [0; 6])
                .unwrap(),
            42
        );
        assert_eq!(engine.library_reports()[0].sha256, expected_sha);
        let mut pointer = [0; 8];
        engine
            .read(primary.image_base() + 0x2180, &mut pointer)
            .unwrap();
        assert_eq!(u64::from_le_bytes(pointer), 0x1800001000);
    }

    #[test]
    fn dynamic_lookup_resolves_only_mapped_module_and_exact_export() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let dep = fixture(0x1800000000, "answer", None, true);
        let mut engine =
            GuestEngine::load_with_libraries(&primary, &[("C:/runtime/dep.dll", dep)]).unwrap();
        let load = STUB_BASE + 0x100;
        let get = STUB_BASE + 0x110;
        install_win64_import(&mut engine.unicorn, load, "kernel32.dll", "LoadLibraryA").unwrap();
        install_win64_import(&mut engine.unicorn, get, "kernel32.dll", "GetProcAddress").unwrap();
        let name = engine.allocate(128, 8).unwrap();
        engine.write(name, b"c:\\runtime\\DEP.dll\0").unwrap();
        let handle = engine.call_win64(load, [name, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(handle, 0x1800000000);
        engine.write(name, b"answer\0").unwrap();
        let entry = engine.call_win64(get, [handle, name, 0, 0, 0, 0]).unwrap();
        assert_eq!(engine.call_win64(entry, [0; 6]).unwrap(), 42);
        engine.write(name, b"missing\0").unwrap();
        assert_eq!(
            engine.call_win64(get, [handle, name, 0, 0, 0, 0]).unwrap(),
            0
        );
        engine.write(name, b"C:/other/dep.dll\0").unwrap();
        assert_eq!(engine.call_win64(load, [name, 0, 0, 0, 0, 0]).unwrap(), 0);
    }

    #[test]
    fn free_library_releases_logical_reference_but_keeps_manifest_module_pinned() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let dep = fixture(0x1800000000, "answer", None, true);
        let mut engine =
            GuestEngine::load_with_libraries(&primary, &[("C:/runtime/dep.dll", dep)]).unwrap();
        let free = STUB_BASE + 0x100;
        let get = STUB_BASE + 0x110;
        let load = STUB_BASE + 0x120;
        install_win64_import(&mut engine.unicorn, free, "kernel32.dll", "FreeLibrary").unwrap();
        install_win64_import(&mut engine.unicorn, get, "kernel32.dll", "GetProcAddress").unwrap();
        install_win64_import(&mut engine.unicorn, load, "kernel32.dll", "LoadLibraryA").unwrap();
        let name = engine.allocate(32, 8).unwrap();
        let export_name = engine.allocate(32, 8).unwrap();
        engine.write(name, b"dep.dll\0").unwrap();
        engine.write(export_name, b"answer\0").unwrap();
        assert_eq!(
            engine.call_win64(load, [name, 0, 0, 0, 0, 0]).unwrap(),
            0x1800000000
        );
        assert_eq!(
            engine
                .call_win64(free, [0x1800000000, 0, 0, 0, 0, 0])
                .unwrap(),
            1
        );
        let entry = engine
            .call_win64(get, [0x1800000000, export_name, 0, 0, 0, 0])
            .unwrap();
        assert_eq!(engine.call_win64(entry, [0; 6]).unwrap(), 42);
        assert_eq!(
            engine
                .call_win64(free, [0x1800000000, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }

    #[test]
    fn exception_type_names_resolve_within_the_owning_dll() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let dep = fixture(0x1800000000, "answer", None, true);
        let mut engine = GuestEngine::load_with_libraries(&primary, &[("dep.dll", dep)]).unwrap();
        let base = 0x1800000000;
        engine
            .write(base + 0x220c, &0x2240u32.to_le_bytes())
            .unwrap();
        engine.write(base + 0x2240, &1u32.to_le_bytes()).unwrap();
        engine
            .write(base + 0x2244, &0x2260u32.to_le_bytes())
            .unwrap();
        engine
            .write(base + 0x2264, &0x2280u32.to_le_bytes())
            .unwrap();
        engine
            .write(base + 0x2290, b".?AVbad_alloc@std@@\0")
            .unwrap();
        assert_eq!(
            msvc_throw_type_name(&engine.unicorn, base + 0x2200).as_deref(),
            Some(".?AVbad_alloc@std@@")
        );
        engine
            .write(base + 0x2264, &u32::MAX.to_le_bytes())
            .unwrap();
        assert_eq!(msvc_throw_type_name(&engine.unicorn, base + 0x2200), None);
        engine
            .write(base + 0x2264, &0x2280u32.to_le_bytes())
            .unwrap();
        engine
            .unicorn
            .mem_protect(base + 0x2000, PAGE_SIZE, Prot::WRITE)
            .unwrap();
        assert_eq!(msvc_throw_type_name(&engine.unicorn, base + 0x2200), None);
        assert_eq!(msvc_throw_type_name(&engine.unicorn, 0), None);
    }

    #[test]
    fn load_ex_a_uses_loaded_basename_before_system32_search() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let dep = fixture(0x1800000000, "answer", None, true);
        let mut engine =
            GuestEngine::load_with_libraries(&primary, &[("C:/runtime/dep.dll", dep)]).unwrap();
        let load = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, load, "kernel32.dll", "LoadLibraryExA").unwrap();
        let name = engine.allocate(128, 8).unwrap();
        for path in [b"DEP\0".as_slice(), b"C:/runtime/dep.dll\0"] {
            engine.write(name, path).unwrap();
            assert_eq!(
                engine.call_win64(load, [name, 0, 0x800, 0, 0, 0]).unwrap(),
                0x1800000000
            );
        }
        engine.write(name, b"C:/other/dep.dll\0").unwrap();
        assert_eq!(
            engine.call_win64(load, [name, 0, 0x800, 0, 0, 0]).unwrap(),
            0
        );
    }

    #[test]
    fn mapped_module_queries_resolve_names_addresses_and_paths() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let dep = fixture(0x1800000000, "answer", None, true);
        let mut engine =
            GuestEngine::load_with_libraries(&primary, &[("C:/runtime/dep.dll", dep)]).unwrap();
        let output = engine.allocate(256, 8).unwrap();
        let name = engine.allocate(128, 8).unwrap();
        let stub = STUB_BASE + 0x100;
        for (index, api) in [
            "GetModuleHandleExA",
            "GetModuleHandleExW",
            "GetModuleHandleW",
            "GetModuleFileNameW",
            "RtlPcToFileHeader",
        ]
        .iter()
        .enumerate()
        {
            install_win64_import(
                &mut engine.unicorn,
                stub + index as u64 * 16,
                "kernel32.dll",
                api,
            )
            .unwrap();
        }
        for entry in [stub, stub + 16] {
            assert_eq!(
                engine
                    .call_win64(entry, [6, 0x1800001000, output, 0, 0, 0])
                    .unwrap(),
                1
            );
            let mut value = [0; 8];
            engine.read(output, &mut value).unwrap();
            assert_eq!(u64::from_le_bytes(value), 0x1800000000);
            assert_eq!(
                engine
                    .call_win64(
                        entry,
                        [6, 0x1800001000, primary.image_base() + 0x1000, 0, 0, 0]
                    )
                    .unwrap(),
                0
            );
        }
        engine.write(name, b"DEP.dll\0").unwrap();
        assert_eq!(
            engine.call_win64(stub, [2, name, output, 0, 0, 0]).unwrap(),
            1
        );
        let wide: Vec<u8> = "DEP.dll\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        engine.write(name, &wide).unwrap();
        assert_eq!(
            engine
                .call_win64(stub + 16, [2, name, output, 0, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.call_win64(stub + 32, [name, 0, 0, 0, 0, 0]).unwrap(),
            0x1800000000
        );
        assert_eq!(
            engine
                .call_win64(stub + 64, [0x1800001000, output, 0, 0, 0, 0])
                .unwrap(),
            0x1800000000
        );
        let expected: Vec<u8> = "c:\\runtime\\dep.dll\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            engine
                .call_win64(stub + 48, [0x1800000000, output, 128, 0, 0, 0])
                .unwrap(),
            (expected.len() / 2 - 1) as u64
        );
        let mut actual = vec![0; expected.len()];
        engine.read(output, &mut actual).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn precomputed_runtime_sync_preserves_lower_lanes_and_clears_upper_lanes_in_real_dll() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let mut bytes = fixture_bytes(0x1800000000, "answer", None, false);
        bytes[0x200..0x205].copy_from_slice(&[0x67, 0xc5, 0xf8, 0x77, 0xc3]);
        let dep = PeImage::parse_library(&bytes).unwrap();
        let mut engine = GuestEngine::load_with_libraries(&primary, &[("dep.dll", dep)]).unwrap();
        for index in 0..16 {
            engine
                .unicorn
                .reg_write_long(unicorn_ymm_register(index).unwrap(), &[index as u8 + 1; 32])
                .unwrap();
        }
        engine.call_win64(0x1800001000, [0; 6]).unwrap();
        for index in 0..16 {
            let value = engine
                .unicorn
                .reg_read_long(unicorn_ymm_register(index).unwrap())
                .unwrap();
            assert_eq!(&value[..16], [index as u8 + 1; 16]);
            assert_eq!(&value[16..], [0; 16]);
        }
        assert_eq!(aex_unicorn_buffer::x86_avx_defined_mask(&engine.unicorn), 0xffff);
    }

    #[test]
    fn modules_receive_distinct_static_tls_indices_and_templates() {
        let make = |base, symbol, byte| {
            let mut bytes = fixture_bytes(base, symbol, None, false);
            bytes[0x150..0x154].copy_from_slice(&0x2240u32.to_le_bytes());
            bytes[0x154..0x158].copy_from_slice(&40u32.to_le_bytes());
            for (offset, address) in [(0, base + 0x22c0), (8, base + 0x22c2), (16, base + 0x22b0)] {
                bytes[0x640 + offset..0x648 + offset].copy_from_slice(&address.to_le_bytes());
            }
            bytes[0x660..0x664].copy_from_slice(&2u32.to_le_bytes());
            bytes[0x6c0..0x6c2].fill(byte);
            PeImage::parse_library(&bytes).unwrap()
        };
        let primary = make(0x180000000, "EffectMain", 0x11);
        let dep = make(0x1800000000, "answer", 0x22);
        let engine = GuestEngine::load_with_libraries(&primary, &[("dep.dll", dep)]).unwrap();
        let mut pointer = [0; 8];
        engine.read(0x58, &mut pointer).unwrap();
        let array = u64::from_le_bytes(pointer);
        let mut blocks = Vec::new();
        for (index, base, byte) in [(0u32, 0x180000000, 0x11), (1, 0x1800000000, 0x22)] {
            let mut slot = [0; 4];
            engine.read(base + 0x22b0, &mut slot).unwrap();
            assert_eq!(u32::from_le_bytes(slot), index);
            engine
                .read(array + u64::from(index) * 8, &mut pointer)
                .unwrap();
            let block = u64::from_le_bytes(pointer);
            blocks.push(block);
            engine.read(block, &mut slot).unwrap();
            assert_eq!(slot, [byte, byte, 0, 0]);
        }
        assert_ne!(blocks[0], blocks[1]);
    }

    #[test]
    fn rejects_cycles_and_dll_attach_failure() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let a = fixture(0x1800000000, "answer", Some(("b.dll", "answer")), true);
        let b = fixture(0x1900000000, "answer", Some(("a.dll", "answer")), true);
        let error = GuestEngine::load_with_libraries(&primary, &[("a.dll", a), ("b.dll", b)])
            .err()
            .unwrap();
        assert!(error.to_string().contains("cyclic DLL"));
        let mut bytes = fixture_bytes(0x1800000000, "answer", None, true);
        bytes[0x240..0x243].copy_from_slice(&[0x31, 0xc0, 0xc3]);
        let dep = PeImage::parse_library(&bytes).unwrap();
        let error = GuestEngine::load_with_libraries(&primary, &[("dep.dll", dep)])
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("DLL initialization dep.dll sha256=")
        );
    }

    #[test]
    fn deferred_primary_attach_runs_only_after_dependencies_are_ready() {
        let mut primary_bytes = fixture_bytes(0x180000000, "EffectMain", None, true);
        // DllMain writes a witness byte into the primary data section, then
        // returns TRUE. The deferred template must not expose that mutation.
        primary_bytes[0x240..0x24d].copy_from_slice(&[
            0xc6, 0x05, 0x39, 0x10, 0x00, 0x00, 0x7a, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xc3,
        ]);
        let primary = PeImage::parse_and_map(&primary_bytes).unwrap();
        let mut other_bytes = primary_bytes.clone();
        other_bytes[0x246] = 0x7b;
        let other = PeImage::parse_and_map(&other_bytes).unwrap();
        let dependency = fixture(DEPENDENCY_IMAGE_BASE, "answer", None, true);
        let mut engine = GuestEngine::load_with_libraries_deferred_primary(
            &primary,
            &[("dep.dll", dependency)],
        )
        .unwrap();
        let mut witness = [0u8];
        engine.read(primary.image_base() + 0x2080, &mut witness).unwrap();
        assert_eq!(witness, [0]);
        assert!(engine
            .unicorn
            .get_data()
            .loaded_libraries
            .values()
            .all(|library| library.initialized));
        assert!(engine.validate_attached_primary(&primary).is_err());
        assert!(engine.attach_deferred_primary(&other).is_err());

        engine.attach_deferred_primary(&primary).unwrap();
        engine.read(primary.image_base() + 0x2080, &mut witness).unwrap();
        assert_eq!(witness, [0x7a]);
        assert!(engine.validate_attached_primary(&primary).is_ok());
        assert!(engine.validate_attached_primary(&other).is_err());
        assert!(engine.attach_deferred_primary(&primary).is_err());

        let detached = GuestEngine::load_with_libraries_deferred_primary(
            &primary,
            &[(
                "dep.dll",
                fixture(DEPENDENCY_IMAGE_BASE, "answer", None, true),
            )],
        )
        .unwrap();
        assert!(crate::classic::ClassicHost::from_engine_with_effect(detached, &primary, None)
            .is_err());

        let mut failing_bytes = fixture_bytes(0x180000000, "EffectMain", None, true);
        failing_bytes[0x240..0x243].copy_from_slice(&[0x31, 0xc0, 0xc3]);
        let failing = PeImage::parse_and_map(&failing_bytes).unwrap();
        let mut poisoned = GuestEngine::load_with_libraries_deferred_primary(
            &failing,
            &[(
                "dep.dll",
                fixture(DEPENDENCY_IMAGE_BASE, "answer", None, true),
            )],
        )
        .unwrap();
        assert!(poisoned.attach_deferred_primary(&failing).is_err());
        assert!(poisoned.attach_deferred_primary(&failing).is_err());
        assert!(poisoned.validate_attached_primary(&failing).is_err());
    }

    #[test]
    fn manifest_parallel_parse_preserves_declared_order_and_first_error() {
        struct TemporaryDirectory(std::path::PathBuf);
        impl Drop for TemporaryDirectory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let unique = format!(
            "aexcompat-library-manifest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let directory = TemporaryDirectory(std::env::temp_dir().join(unique));
        std::fs::create_dir(&directory.0).unwrap();
        std::fs::write(
            directory.0.join("first.dll"),
            fixture_bytes(0x1800000000, "first", None, true),
        )
        .unwrap();
        std::fs::write(
            directory.0.join("second.dll"),
            fixture_bytes(DEPENDENCY_IMAGE_BASE + 0x10000, "second", None, true),
        )
        .unwrap();
        let manifest_path = directory.0.join("libraries.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&serde_json::json!({"libraries": [
                {"name": "first.dll", "path": "first.dll"},
                {"name": "second.dll", "path": "second.dll"}
            ]}))
            .unwrap(),
        )
        .unwrap();
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let engine = GuestEngine::load_with_library_manifest(&primary, &manifest_path).unwrap();
        assert_eq!(
            guest_library_by_name(engine.unicorn.get_data(), "first.dll")
                .unwrap()
                .base,
            DEPENDENCY_IMAGE_BASE
        );
        assert!(
            guest_library_by_name(engine.unicorn.get_data(), "second.dll")
                .unwrap()
                .base
                > DEPENDENCY_IMAGE_BASE
        );

        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&serde_json::json!({"libraries": [
                {"name": "first-missing.dll", "path": "missing-1.dll"},
                {"name": "second-missing.dll", "path": "missing-2.dll"}
            ]}))
            .unwrap(),
        )
        .unwrap();
        let error = GuestEngine::load_with_library_manifest(&primary, &manifest_path)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("first-missing.dll"), "{error}");
    }

    #[test]
    fn manifest_parse_batch_charges_worst_case_images_before_parallel_work() {
        assert_eq!(dependency_parse_batch_len(64, 4), 4);
        assert_eq!(dependency_parse_batch_len(3, 4), 3);
        assert_eq!(dependency_parse_batch_len(1, 4), 1);
        assert_eq!(dependency_parse_batch_len(0, 4), 0);
    }

    #[test]
    fn dependency_images_leave_lazy_heap_space_available() {
        let primary = fixture(0x180000000, "EffectMain", None, false);
        let colliding = fixture(CRT_HEAP_BASE, "answer", None, false);
        assert!(
            GuestEngine::load_with_libraries(&primary, &[("dep.dll", colliding)])
                .err()
                .unwrap()
                .to_string()
                .contains("reserved guest address")
        );
        let dep = fixture(DEPENDENCY_IMAGE_BASE, "answer", None, false);
        let mut engine = GuestEngine::load_with_libraries(&primary, &[("dep.dll", dep)]).unwrap();
        let pointer =
            allocate_process_heap_region(&mut engine.unicorn, PROCESS_HEAP_HANDLE, 968).unwrap();
        assert_eq!(pointer, CRT_HEAP_BASE);
        free_process_heap_region(&mut engine.unicorn, PROCESS_HEAP_HANDLE, pointer).unwrap();
    }

    #[test]
    fn rejects_overlapping_images_missing_exports_and_ambiguous_names() {
        let primary = fixture(0x180000000, "EffectMain", Some(("dep.dll", "answer")), true);
        let overlap_a = fixture(DEPENDENCY_IMAGE_BASE, "answer", None, true);
        let overlap_b = fixture(DEPENDENCY_IMAGE_BASE, "answer", None, true);
        let collision = GuestEngine::load_with_libraries(
            &primary,
            &[("dep.dll", overlap_a), ("other.dll", overlap_b)],
        )
        .err()
        .unwrap();
        assert!(
            collision.to_string().contains("map dependency DLL"),
            "{collision}"
        );
        let missing = fixture(0x1800000000, "wrong", None, true);
        assert!(GuestEngine::load_with_libraries(&primary, &[("dep.dll", missing)]).is_err());
        let a = fixture(0x1800000000, "answer", None, true);
        let b = fixture(0x1900000000, "answer", None, true);
        assert!(
            GuestEngine::load_with_libraries(&primary, &[("A/dep.dll", a), ("B/dep.dll", b)])
                .is_err()
        );
    }
    #[test]
    fn imported_cpp_data_is_shared_writable_and_not_executable() {
        let make = |base, symbol| {
            let mut bytes = fixture_bytes(
                base,
                symbol,
                Some(("msvcp140.dll", "?_Index@ios_base@std@@0HA")),
                true,
            );
            // DllMain increments the imported int and returns TRUE.
            bytes[0x240..0x24f].copy_from_slice(&[
                0x48, 0x8b, 0x05, 0x39, 0x11, 0, 0, 0xff, 0x00, 0xb8, 1, 0, 0, 0, 0xc3,
            ]);
            // Export increments the same imported int and returns its value.
            bytes[0x200..0x20c].copy_from_slice(&[
                0x48, 0x8b, 0x05, 0x79, 0x11, 0, 0, 0xff, 0x00, 0x8b, 0x00, 0xc3,
            ]);
            PeImage::parse_library(&bytes).unwrap()
        };
        let primary = make(0x180000000, "EffectMain");
        let dep = make(DEPENDENCY_IMAGE_BASE, "answer");
        let mut engine = GuestEngine::load_with_libraries(&primary, &[("dep.dll", dep)]).unwrap();
        let mut primary_iat = [0; 8];
        let mut dep_iat = [0; 8];
        engine
            .read(primary.image_base() + 0x2180, &mut primary_iat)
            .unwrap();
        engine
            .read(DEPENDENCY_IMAGE_BASE + 0x2180, &mut dep_iat)
            .unwrap();
        assert_eq!(primary_iat, dep_iat);
        let address = u64::from_le_bytes(primary_iat);
        assert!(
            guest_range_has_permission(&engine.unicorn, address, 4, Prot::READ | Prot::WRITE)
                .unwrap()
        );
        assert!(!guest_range_has_permission(&engine.unicorn, address, 1, Prot::EXEC).unwrap());
        assert_eq!(
            engine
                .call_win64(primary.image_base() + 0x1000, [0; 6])
                .unwrap(),
            3
        );
        assert_eq!(
            engine
                .call_win64(DEPENDENCY_IMAGE_BASE + 0x1000, [0; 6])
                .unwrap(),
            4
        );
        assert!(!guest_range_has_permission(&engine.unicorn, STUB_BASE, 1, Prot::WRITE).unwrap());
        let mut independent = GuestEngine::load_with_libraries(&primary, &[]).unwrap();
        assert_eq!(
            independent
                .call_win64(primary.image_base() + 0x1000, [0; 6])
                .unwrap(),
            2
        );
        let sync = engine
            .resolve_emulated_import_data("MSVCP140.DLL", "?_Sync@ios_base@std@@0_NA")
            .unwrap()
            .unwrap();
        let mut byte = [0];
        engine.read(sync, &mut byte).unwrap();
        assert_eq!(byte, [1]);
        assert!(
            engine
                .resolve_emulated_import_data("fixture.dll", "?_Index@ios_base@std@@0HA")
                .unwrap()
                .is_none()
        );
    }
}
