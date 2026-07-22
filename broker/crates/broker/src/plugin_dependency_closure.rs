//! Resolves the dependency-DLL closure an AEX needs inside the sealed load tree
//! (issue #304).
//!
//! The worker loads the plug-in with
//! `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32`, and the
//! broker stages the plug-in into a sealed root that holds only what the caller
//! declared as dependencies. A plug-in whose imports live next to it in its
//! install folder (an After Effects effect importing `dvacore.dll` from the AE
//! `Support Files\` folder) therefore fails the worker's module load with exit
//! code 11: the isolated root has no such neighbour, so the Windows image loader
//! cannot resolve its imports. This module walks the plug-in's PE import tables,
//! resolves each imported name against caller-supplied search roots, and returns
//! the closure as dependencies the sealed tree can carry.
//!
//! What this module does *not* do:
//!
//! - It never widens the worker's DLL search path. Every module it returns goes
//!   through the same `session_dependency_manifest` authentication a hand-written
//!   dependency does, and is then copied, re-hashed, and pinned by
//!   `SealedLoadTree`.
//! - It resolves only what the PE import tables name. A module the plug-in loads
//!   later by absolute path at runtime, rather than through its import tables,
//!   is invisible here and stays an unknown module in the worker's module audit.
//! - A name only reaches the closure if a search root provides it. Roots are
//!   consulted in order, matching `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR` before
//!   `LOAD_LIBRARY_SEARCH_SYSTEM32`, so an app-local runtime a plug-in ships
//!   beside itself is sealed rather than the same-named System32 file. Anything
//!   no root provides is left to the loader and reported, not sealed.
//! - It does not model what the loader resolves *before* it searches directories:
//!   the API set schema (which is why `api-ms-*` / `ext-ms-*` names are never
//!   sealed), the KnownDLLs section, and the modules already mapped into the
//!   worker. A root-provided copy of `kernel32.dll`, or of a CRT the worker has
//!   already loaded, is therefore sealed and then never used. That costs a copy;
//!   it does not change which module loads, and the sealed tree stays a superset
//!   of what the plug-in needs.

use crate::secure_image_dispatch::ApprovedImageArtifact;
use crate::session_dependency_manifest::{
    SessionDependencyDto, SessionDependencyManifestDto, validate_with_limit,
};
use object::LittleEndian;
use object::read::pe::{ImageNtHeaders, PeFile32, PeFile64};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Upper bound on the search roots one resolution may consult. Roots are tried
/// in order and the first match wins, which mirrors how the Windows loader
/// resolves one name once; a long root list would only make that order harder
/// to reason about.
pub const MAX_SEARCH_ROOTS: usize = 8;

/// Largest plug-in or dependency image this resolver will parse for imports.
const MAX_PARSED_IMAGE_BYTES: u64 = 512 * 1_024 * 1_024;

/// Import descriptors this resolver walks per image, guarding a hostile import
/// table.
const MAX_IMPORT_NAMES_PER_IMAGE: usize = 4_096;

/// Longest imported name kept. A DLL name is a filename, so `MAX_PATH` is
/// already generous; without a bound, one descriptor can name a run of bytes as
/// long as its section, and every descriptor in the table can name the same run
/// at a different offset, so retained memory is descriptors times run length
/// rather than anything the file size bounds.
const MAX_IMPORT_NAME_BYTES: usize = 260;

/// Imported names one closure may decide about, across every image in it. Bounds
/// what the walk retains (`seen`, the unresolved set) for a plug-in whose graph
/// is wide rather than deep.
const MAX_CLOSURE_IMPORT_NAMES: usize = 16_384;

#[derive(Clone, Debug)]
pub struct DependencyClosureRequest<'a> {
    /// The plug-in whose imports are walked. Its own bytes are never returned as
    /// a dependency.
    pub plugin: &'a Path,
    /// Folders searched, in order, for each imported name. Only direct children
    /// are considered; the resolver never descends into subfolders.
    pub search_roots: &'a [PathBuf],
    /// Optional ceiling on how many modules may be sealed. `None` seals the whole
    /// closure.
    ///
    /// There is deliberately no default ceiling. The closure is not
    /// caller-supplied data: it is derived from the plug-in's own import tables
    /// and can only name files that already exist as direct children of the
    /// operator's search roots, so its size is bounded by what the operator
    /// pointed the resolver at. A fixed ceiling here would reject a plug-in for
    /// needing a large runtime rather than for anything unsafe, and sealing is
    /// what makes such a plug-in loadable at all. Callers that would rather fail
    /// than pay the copy set this.
    pub max_dependencies: Option<usize>,
    /// Optional ceiling on the total resolved bytes. `None` seals the whole
    /// closure; see `max_dependencies` for why that is the default.
    pub max_total_bytes: Option<u64>,
}

impl<'a> DependencyClosureRequest<'a> {
    pub fn new(plugin: &'a Path, search_roots: &'a [PathBuf]) -> Self {
        Self {
            plugin,
            search_roots,
            max_dependencies: None,
            max_total_bytes: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDependencyClosure {
    dependencies: Vec<ApprovedImageArtifact>,
    unresolved: Vec<String>,
    rejected_names: usize,
    total_bytes: u64,
}

impl ResolvedDependencyClosure {
    /// The authenticated dependencies, ready to be sealed.
    pub fn dependencies(&self) -> &[ApprovedImageArtifact] {
        &self.dependencies
    }

    pub fn into_dependencies(self) -> Vec<ApprovedImageArtifact> {
        self.dependencies
    }

    /// Imported names none of the search roots provided, lowercased and sorted.
    ///
    /// These are not an error: a name is normally here because it is a Windows
    /// API set or a System32 DLL the loader finds on its own. It is reported so a
    /// load failure can be read against what was left out, and so a caller
    /// caching this result can notice when a root starts providing one of them —
    /// that is the moment the closure would change. Basenames only, never a path.
    pub fn unresolved(&self) -> &[String] {
        &self.unresolved
    }

    /// How many imported names were not even shaped like a DLL basename (a
    /// separator, a drive letter, a reserved device name, non-ASCII bytes).
    ///
    /// Only the count is kept: the name comes from inside the plug-in image, so
    /// echoing it into a diagnostic would let a plug-in write arbitrary text
    /// into a report.
    pub fn rejected_names(&self) -> usize {
        self.rejected_names
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
    }
}

/// Walks `request.plugin`'s import closure and returns the modules that resolve
/// under the search roots, authenticated as sealable dependencies.
///
/// Fails closed rather than truncating: a closure over the count or byte ceiling
/// is an error, because a silently shortened closure would reappear as the same
/// opaque module-load failure this module exists to remove.
pub fn resolve_dependency_closure(
    request: DependencyClosureRequest<'_>,
) -> io::Result<ResolvedDependencyClosure> {
    let plugin = validated_image_path(request.plugin)?;
    let walk = walk_import_closure(
        &plugin,
        request.search_roots,
        request.max_dependencies,
        request.max_total_bytes,
        UnreadableImage::Fail,
        ImageIdentity::Bind,
    )?;
    if walk.over_module_limit {
        return Err(invalid("dependency closure module limit exceeded"));
    }
    if walk.over_byte_limit {
        return Err(invalid("dependency closure byte limit exceeded"));
    }
    // Same binding as for the dependencies, at the root of the graph: the
    // identity handed to authentication is the digest of the bytes this walk
    // expanded, so a plug-in rewritten mid-walk cannot be sealed with an import
    // list it no longer has.
    let plugin_artifact = walk
        .plugin
        .as_ref()
        .and_then(|image| {
            Some(ApprovedImageArtifact {
                path: image.path.clone(),
                expected_sha256: image.sha256?,
                expected_size: image.size,
            })
        })
        .ok_or_else(|| invalid("plug-in identity was not bound"))?;

    // The identity declared here is the digest of the bytes the walk parsed, so
    // the manifest's re-read below is what binds "the imports we followed" to
    // "the bytes that get sealed": a dependency rewritten between the two fails
    // authentication instead of being sealed with someone else's import list.
    let dependencies = walk
        .resolved
        .iter()
        .map(|image| {
            let sha256 = image
                .sha256
                .ok_or_else(|| invalid("dependency identity was not bound"))?;
            Ok(SessionDependencyDto {
                basename: basename_of(&image.path)?,
                path: image.path.clone(),
                sha256: hex(&sha256),
                size: image.size,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    // Authentication is the session dependency manifest's job, not this
    // resolver's: it re-reads every file, rejects reparse points, and enforces
    // the basename collision rules the sealed tree depends on. Its own count
    // limit is a bound on an externally supplied JSON document, so it does not
    // apply to this list, which the broker derived from the plug-in's imports.
    let validated = validate_with_limit(
        SessionDependencyManifestDto {
            schema_version: 1,
            dependencies,
        },
        &plugin_artifact,
        walk.resolved.len(),
    )?;
    Ok(ResolvedDependencyClosure {
        dependencies: validated.into_approved_image_artifacts(),
        unresolved: walk.unresolved,
        rejected_names: walk.rejected_names,
        total_bytes: walk.total_bytes,
    })
}

/// What an import-closure walk found, without authenticating anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyClosureSurvey {
    /// The dependency files the walk reached, in resolution order. A caller that
    /// could not seal them (over its own ceiling, say) can still record their
    /// identities and notice when they change.
    pub modules: Vec<PathBuf>,
    pub total_bytes: u64,
    pub unresolved: Vec<String>,
    pub rejected_names: usize,
    /// Images whose import table could not be read. A survey reports them and
    /// keeps going — it measures, it never feeds a dispatch, and stopping at the
    /// first unreadable image would leave a caller with nothing at all where it
    /// could have had most of the closure.
    pub unreadable_images: usize,
}

/// Counts what `plugin`'s import closure would seal, without hashing or
/// authenticating it.
///
/// This is the measurement counterpart of `resolve_dependency_closure`: it
/// answers "how big is this plug-in's closure" (how many modules and bytes one
/// dispatch would copy into the sealed tree) without paying for the copy.
/// Nothing it returns may be dispatched.
pub fn survey_dependency_closure(
    plugin: &Path,
    search_roots: &[PathBuf],
) -> io::Result<DependencyClosureSurvey> {
    let plugin = validated_image_path(plugin)?;
    let walk = walk_import_closure(
        &plugin,
        search_roots,
        None,
        None,
        UnreadableImage::KeepGoing,
        ImageIdentity::Measure,
    )?;
    Ok(DependencyClosureSurvey {
        modules: walk.resolved.into_iter().map(|image| image.path).collect(),
        total_bytes: walk.total_bytes,
        unresolved: walk.unresolved,
        rejected_names: walk.rejected_names,
        unreadable_images: walk.unreadable_images,
    })
}

/// A dependency the walk reached, identified by the bytes the walk itself read.
#[derive(Clone, Debug, Eq, PartialEq)]
struct WalkedImage {
    path: PathBuf,
    size: u64,
    /// SHA-256 of the bytes the imports were parsed from. `None` when the walk
    /// was only measuring and did not hash.
    sha256: Option<[u8; 32]>,
}

struct ImportClosureWalk {
    /// The plug-in itself, identified by the bytes its imports were parsed from.
    /// `None` only when a measuring walk could not read it.
    plugin: Option<WalkedImage>,
    resolved: Vec<WalkedImage>,
    unresolved: Vec<String>,
    rejected_names: usize,
    unreadable_images: usize,
    total_bytes: u64,
    over_module_limit: bool,
    over_byte_limit: bool,
}

/// Whether a walk hashes the bytes it parses.
#[derive(Clone, Copy, Eq, PartialEq)]
enum ImageIdentity {
    /// Hash them. Anything that will be sealed must be approved as *these* bytes,
    /// not as whatever the file holds by the time it is re-read: a dependency
    /// rewritten mid-walk would otherwise be sealed with imports parsed from the
    /// bytes it no longer has, and the closure could be missing what the new
    /// bytes need.
    Bind,
    /// Skip hashing. Only for measurement, which seals nothing.
    Measure,
}

/// What a walk does with an image whose import table it cannot read.
#[derive(Clone, Copy, Eq, PartialEq)]
enum UnreadableImage {
    /// Fail the whole walk. Anything that will be sealed must be complete: a
    /// dropped dependency becomes an opaque module-load failure later.
    Fail,
    /// Count it and carry on. Only for measurement, which never dispatches.
    KeepGoing,
}

/// Breadth-first walk of `plugin`'s import graph, resolving each imported name
/// once. Stops as soon as a ceiling is crossed and says so, leaving the caller
/// to decide whether that is an error or a measurement.
fn walk_import_closure(
    plugin: &Path,
    search_roots: &[PathBuf],
    max_modules: Option<usize>,
    max_total_bytes: Option<u64>,
    unreadable: UnreadableImage,
    identity: ImageIdentity,
) -> io::Result<ImportClosureWalk> {
    if search_roots.len() > MAX_SEARCH_ROOTS {
        return Err(invalid("dependency search root limit exceeded"));
    }
    let roots = canonical_search_roots(search_roots)?;
    // `seen` folds every imported name the walk has already decided about, so a
    // diamond in the import graph is visited once. The plug-in's own basename is
    // seeded so a self-referencing import cannot re-seal the plug-in.
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(fold(&basename_of(plugin)?));
    let mut unresolved: HashSet<String> = HashSet::new();
    let mut walk = ImportClosureWalk {
        plugin: None,
        resolved: Vec::new(),
        unresolved: Vec::new(),
        rejected_names: 0,
        unreadable_images: 0,
        total_bytes: 0,
        over_module_limit: false,
        over_byte_limit: false,
    };
    // The queue holds one entry per *unique, validated* name, so what it retains
    // is bounded by the same ceiling `seen` is: queueing whole import lists would
    // let a hostile table (4096 descriptors naming the same string) multiply into
    // orders of magnitude more memory than the file it came from.
    let mut queue: VecDeque<String> = VecDeque::new();
    let enqueue = |queue: &mut VecDeque<String>,
                   seen: &mut HashSet<String>,
                   walk: &mut ImportClosureWalk,
                   names: Vec<String>|
     -> io::Result<()> {
        for name in names {
            // Names are ASCII-validated before anything else, so a name that
            // cannot be a DLL basename never reaches `seen`. Folding into `seen`
            // first would let a name that merely *lowercases* onto a real one
            // (U+212A KELVIN SIGN onto `k`, say) claim its key and then be
            // rejected, dropping the real import from the closure in silence.
            if !windows_safe_basename(&name) {
                walk.rejected_names += 1;
                continue;
            }
            if !seen.insert(fold(&name)) {
                continue;
            }
            if seen.len() > MAX_CLOSURE_IMPORT_NAMES {
                return Err(invalid("dependency closure name limit exceeded"));
            }
            queue.push_back(name);
        }
        Ok(())
    };

    // The plug-in is read the same way its dependencies are, so the identity the
    // caller seals is the identity of the bytes these imports came from.
    match read_image_imports(plugin, identity) {
        Ok((image, names)) => {
            walk.plugin = Some(image);
            enqueue(&mut queue, &mut seen, &mut walk, names)?;
        }
        Err(error) if unreadable == UnreadableImage::KeepGoing => {
            walk.unreadable_images += 1;
            let _ = error;
        }
        Err(error) => return Err(error),
    }

    while let Some(name) = queue.pop_front() {
        match resolve_name(&name, &roots)? {
            NameResolution::NotInRoots => {
                unresolved.insert(fold(&name));
            }
            NameResolution::Found(path) => {
                // Ceilings are applied before the image is read: an operator who
                // capped the closure is told they hit the cap, not whatever the
                // first image past it happens to be.
                let size = fs::metadata(&path)?.len();
                walk.over_module_limit =
                    max_modules.is_some_and(|limit| walk.resolved.len() + 1 > limit);
                walk.over_byte_limit = max_total_bytes
                    .is_some_and(|limit| walk.total_bytes.saturating_add(size) > limit);
                if walk.over_module_limit || walk.over_byte_limit {
                    break;
                }
                match read_image_imports(&path, identity) {
                    Ok((image, imports)) => {
                        walk.total_bytes = walk.total_bytes.saturating_add(image.size);
                        walk.resolved.push(image);
                        enqueue(&mut queue, &mut seen, &mut walk, imports)?;
                    }
                    Err(error) if unreadable == UnreadableImage::KeepGoing => {
                        walk.unreadable_images += 1;
                        walk.total_bytes = walk.total_bytes.saturating_add(size);
                        walk.resolved.push(WalkedImage {
                            path,
                            size,
                            sha256: None,
                        });
                        let _ = error;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }
    walk.unresolved = unresolved.into_iter().collect();
    walk.unresolved.sort();
    Ok(walk)
}

enum NameResolution {
    /// Resolved to a direct child of one search root, so it is sealed.
    Found(PathBuf),
    /// No search root provides it. Whether the loader then finds it in System32,
    /// resolves it from the API set schema, or fails is not this resolver's
    /// business: either way nothing is sealed, and either way a root that starts
    /// providing the name later would change the closure.
    NotInRoots,
}

fn resolve_name(name: &str, roots: &[PathBuf]) -> io::Result<NameResolution> {
    // An API set name is resolved by the loader from the API set schema before
    // any directory is searched, so a copy sitting in a search root would never
    // be the module that loads. Leave it to the loader rather than sealing a file
    // the worker will ignore — and rather than failing the whole closure over one.
    if is_api_set_name(name) {
        return Ok(NameResolution::NotInRoots);
    }
    // Search roots are consulted in order, matching
    // `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR` before `LOAD_LIBRARY_SEARCH_SYSTEM32`:
    // an app-local runtime the plug-in ships deliberately (Adobe's own
    // `msvcp140.dll` next to its effects, say) must be the copy that is sealed,
    // not the same-named System32 file.
    for root in roots {
        if is_direct_child_file(root, name) {
            let candidate = root.join(name);
            let canonical = validated_image_path(&candidate)?;
            if canonical.parent() != Some(root.as_path()) {
                return Err(invalid("dependency resolved outside its search root"));
            }
            if !basename_of(&canonical)?.eq_ignore_ascii_case(name) {
                return Err(invalid("dependency resolved to a different basename"));
            }
            return Ok(NameResolution::Found(canonical));
        }
    }
    Ok(NameResolution::NotInRoots)
}

fn is_direct_child_file(root: &Path, name: &str) -> bool {
    let candidate = root.join(name);
    candidate.parent() == Some(root) && candidate.is_file()
}

fn canonical_search_roots(roots: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let mut canonical = Vec::with_capacity(roots.len());
    let mut folded = HashSet::with_capacity(roots.len());
    for root in roots {
        if !root.is_absolute() {
            return Err(invalid("dependency search root must be absolute"));
        }
        let resolved = fs::canonicalize(root)?;
        if !resolved.is_dir() {
            return Err(invalid("dependency search root must be a directory"));
        }
        if folded.insert(fold(&resolved.to_string_lossy())) {
            canonical.push(resolved);
        }
    }
    Ok(canonical)
}

fn validated_image_path(path: &Path) -> io::Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    if !canonical.is_file() {
        return Err(invalid("dependency image must be a regular file"));
    }
    Ok(canonical)
}

fn basename_of(path: &Path) -> io::Result<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid("image path must have a UTF-8 basename"))
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    if fs::metadata(path)?.len() > MAX_PARSED_IMAGE_BYTES {
        return Err(invalid("dependency image is too large to authenticate"));
    }
    fs::read(path)
}

/// Reads `image` once and returns both what it imports and the identity of the
/// bytes those imports were read from.
fn read_image_imports(
    image: &Path,
    identity: ImageIdentity,
) -> io::Result<(WalkedImage, Vec<String>)> {
    let bytes = read_bounded(image)?;
    let names = imported_names_from_bytes(&bytes)?;
    Ok((
        WalkedImage {
            path: image.to_path_buf(),
            size: bytes.len() as u64,
            sha256: match identity {
                ImageIdentity::Bind => Some(Sha256::digest(&bytes).into()),
                ImageIdentity::Measure => None,
            },
        },
        names,
    ))
}

/// Every DLL name in `image`'s import and delay-load import tables.
///
/// Delay-loaded names are included because the worker's isolated root is the
/// only place the loader will look for them too, so leaving them out would just
/// move the same failure from load time to first call (issue #60 covers the
/// delay-load execution semantics; this only affects what gets sealed).
fn imported_names_from_bytes(bytes: &[u8]) -> io::Result<Vec<String>> {
    match PeFile64::parse(bytes) {
        Ok(pe) => import_names_from(&pe),
        // Not a PE32+ image: a 32-bit AEX still resolves the same way, and any
        // other content simply contributes no imports.
        Err(_) => match PeFile32::parse(bytes) {
            Ok(pe) => import_names_from(&pe),
            Err(_) => Ok(Vec::new()),
        },
    }
}

fn import_names_from<Nt: ImageNtHeaders>(
    pe: &object::read::pe::PeFile<'_, Nt>,
) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    // The ceiling counts descriptors walked, not names kept: a hostile import
    // directory can hold millions of entries, and counting only what survives
    // would let it decide how long this walk runs. Passing the ceiling is an
    // error, not a truncation, so a closure is never quietly shortened into a
    // load failure either.
    let mut walked = 0usize;
    let step = |names: &mut Vec<String>, walked: &mut usize, raw: Option<&[u8]>| {
        *walked += 1;
        if *walked > MAX_IMPORT_NAMES_PER_IMAGE {
            return Err(invalid("imported name limit exceeded"));
        }
        // A descriptor whose name cannot be read — a bad RVA, empty bytes, more
        // bytes than a filename can hold, or not the ASCII a DLL name is — means
        // the import table is malformed. Skipping it would drop a dependency from
        // the closure and turn a diagnosable parse failure into an opaque
        // module-load failure much later, so it fails here instead.
        let name = raw
            .filter(|raw| !raw.is_empty() && raw.len() <= MAX_IMPORT_NAME_BYTES)
            .and_then(|raw| std::str::from_utf8(raw).ok())
            .ok_or_else(|| invalid("plug-in import name is unreadable"))?;
        names.push(name.to_owned());
        Ok(())
    };
    // A malformed import directory is an error, not "no imports": treating a
    // parse failure as an empty table would seal nothing and hand the worker the
    // same opaque load failure this resolver exists to remove. `Ok(None)` is the
    // honest case of a PE that imports nothing.
    if let Some(table) = pe
        .import_table()
        .map_err(|_| invalid("plug-in import table is unreadable"))?
    {
        let mut descriptors = table
            .descriptors()
            .map_err(|_| invalid("plug-in import table is unreadable"))?;
        while let Some(descriptor) = descriptors
            .next()
            .map_err(|_| invalid("plug-in import table is unreadable"))?
        {
            step(
                &mut names,
                &mut walked,
                name_at(pe, descriptor.name.get(LittleEndian)),
            )?;
        }
    }
    if let Some(table) = pe
        .data_directories()
        .delay_load_import_table(pe.data(), &pe.section_table())
        .map_err(|_| invalid("plug-in delay-load import table is unreadable"))?
    {
        let mut descriptors = table
            .descriptors()
            .map_err(|_| invalid("plug-in delay-load import table is unreadable"))?;
        while let Some(descriptor) = descriptors
            .next()
            .map_err(|_| invalid("plug-in delay-load import table is unreadable"))?
        {
            step(
                &mut names,
                &mut walked,
                name_at(pe, descriptor.dll_name_rva.get(LittleEndian)),
            )?;
        }
    }
    Ok(names)
}

/// The NUL-terminated name at `rva`, resolved against whichever section holds it.
///
/// The import table helper resolves names against one section, chosen from the
/// first descriptor, which is not where every linker puts them: a PE that keeps
/// its descriptors in `.idata` and some of its name strings in `.rdata` makes
/// that helper fail for exactly those descriptors. Two of the 353 After Effects
/// plug-ins measured for issue #304 are built that way, and dropping their
/// imports would have shortened a closure silently. Resolving through the
/// section table covers any layout, and a genuinely out-of-range RVA still fails.
///
/// The terminator is only searched for as far as a name may be long. A name
/// longer than that is rejected by the caller anyway, so scanning past the limit
/// can only ever confirm a rejection — and an RVA aimed at a large section with
/// no NUL in it would otherwise make each of an image's descriptors scan the
/// rest of that section, turning a malformed table into work proportional to
/// descriptors times section size.
fn name_at<'data, Nt: ImageNtHeaders>(
    pe: &object::read::pe::PeFile<'data, Nt>,
    rva: u32,
) -> Option<&'data [u8]> {
    let data = pe.section_table().pe_data_at(pe.data(), rva)?;
    let searched = data.get(..=MAX_IMPORT_NAME_BYTES).unwrap_or(data);
    let end = searched.iter().position(|byte| *byte == 0)?;
    Some(&data[..end])
}

/// Whether `name` is a Windows API set (`api-ms-*` / `ext-ms-*`), which the
/// loader resolves from the API set schema before searching any directory.
fn is_api_set_name(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.starts_with("api-ms-") || folded.starts_with("ext-ms-")
}

/// The subset of names that may be joined to a directory: one plain component,
/// no separators, no drive letter, no control characters, ASCII only (PE import
/// names are ASCII).
fn windows_safe_basename(name: &str) -> bool {
    let mut components = Path::new(name).components();
    if name.is_empty()
        || !name.is_ascii()
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
        || name == "."
        || name == ".."
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    !(matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(stem.as_bytes(), [b'C', b'O', b'M', b'1'..=b'9'])
        || matches!(stem.as_bytes(), [b'L', b'P', b'T', b'1'..=b'9']))
}

fn fold(value: &str) -> String {
    value.to_lowercase()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_pe::pe64_importing;

    fn temp_dir(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-closure-{tag}-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }

    fn write_pe(dir: &Path, name: &str, imports: &[&str]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, pe64_importing(imports)).unwrap();
        path
    }

    #[test]
    fn resolves_the_recursive_closure_and_leaves_system_names_out() {
        let install = temp_dir("install");
        let plugin = write_pe(&install, "effect.aex", &["dvacore.dll", "KERNEL32.dll"]);
        write_pe(&install, "dvacore.dll", &["dvaui.dll", "KERNEL32.dll"]);
        write_pe(&install, "dvaui.dll", &["dvacore.dll"]);

        let roots = vec![install.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();

        let mut sealed: Vec<String> = closure
            .dependencies()
            .iter()
            .map(|artifact| artifact.path.file_name().unwrap().to_string_lossy().into())
            .collect();
        sealed.sort();
        assert_eq!(sealed, vec!["dvacore.dll", "dvaui.dll"]);
        // No root provides kernel32, so it is not sealed — the worker's own load
        // flags reach System32. It is still reported, because a root that starts
        // providing that name would change what the closure seals.
        assert_eq!(closure.unresolved(), ["kernel32.dll"]);
        assert!(closure.total_bytes() > 0);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn a_rejected_name_cannot_take_a_real_import_s_place() {
        // U+212A KELVIN SIGN lowercases to ASCII `k`. If the walk folded names
        // into its visited set before validating them, this name would claim the
        // key for `krt.dll`, then be rejected as non-ASCII, and the real import
        // would never be looked at — a silently shortened closure.
        let install = temp_dir("fold");
        write_pe(&install, "krt.dll", &[]);
        let plugin = write_pe(&install, "effect.aex", &["\u{212a}rt.dll", "krt.dll"]);
        let roots = vec![install.clone()];

        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert_eq!(closure.dependencies().len(), 1);
        assert_eq!(
            closure.dependencies()[0].path.file_name().unwrap(),
            "krt.dll"
        );
        assert_eq!(closure.rejected_names(), 1);
        fs::remove_dir_all(install).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_a_root_entry_that_resolves_outside_its_root() {
        // A junction or symlink planted in a search root is the way a name can
        // pass every basename check and still land somewhere else; the resolver
        // must refuse rather than seal whatever it points at.
        use std::os::windows::fs::symlink_file;
        let install = temp_dir("escape");
        let outside = temp_dir("escape-target");
        let target = write_pe(&outside, "real.dll", &[]);
        let plugin = write_pe(&install, "effect.aex", &["linked.dll"]);
        if symlink_file(&target, install.join("linked.dll")).is_err() {
            // Creating symlinks needs a privilege this machine may not grant.
            fs::remove_dir_all(&install).unwrap();
            fs::remove_dir_all(&outside).unwrap();
            return;
        }

        let roots = vec![install.clone()];
        let error =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "dependency resolved outside its search root"
        );
        fs::remove_dir_all(install).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn never_seals_an_api_set_even_when_a_root_holds_one() {
        // The loader resolves api-ms-* / ext-ms-* from the API set schema before
        // it searches any directory, so an app-local copy would never be the
        // module that loads. Sealing it would copy a file the worker ignores and
        // spend an operator's ceiling on it.
        let install = temp_dir("apiset");
        let api_set = "api-ms-win-crt-runtime-l1-1-0.dll";
        write_pe(&install, api_set, &[]);
        let plugin = write_pe(&install, "effect.aex", &[api_set]);
        let roots = vec![install.clone()];

        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert!(closure.is_empty());
        assert_eq!(closure.unresolved(), [api_set]);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn reports_names_no_search_root_provides() {
        let install = temp_dir("missing");
        let plugin = write_pe(&install, "effect.aex", &["absent-runtime.dll"]);
        let roots = vec![install.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert!(closure.is_empty());
        assert_eq!(closure.unresolved(), ["absent-runtime.dll"]);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn never_seals_the_plugin_itself_or_leaves_its_search_root() {
        let install = temp_dir("self");
        let outside = temp_dir("outside");
        write_pe(&outside, "escaped.dll", &[]);
        let plugin = write_pe(
            &install,
            "effect.aex",
            &["effect.aex", "..\\escaped.dll", "sub/escaped.dll"],
        );
        let roots = vec![install.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert!(closure.is_empty());
        // A traversal-shaped import name is never joined to a directory, so
        // nothing outside the search root is read, and the name itself stays out
        // of the report.
        assert_eq!(closure.rejected_names(), 2);
        assert!(closure.unresolved().is_empty());
        fs::remove_dir_all(install).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn first_search_root_wins_like_the_loader() {
        let preferred = temp_dir("preferred");
        let fallback = temp_dir("fallback");
        let plugin = write_pe(&preferred, "effect.aex", &["shared.dll"]);
        write_pe(&preferred, "shared.dll", &[]);
        fs::write(fallback.join("shared.dll"), b"different bytes").unwrap();

        let roots = vec![preferred.clone(), fallback.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert_eq!(closure.dependencies().len(), 1);
        assert_eq!(
            closure.dependencies()[0].path,
            preferred.join("shared.dll").canonicalize().unwrap()
        );
        fs::remove_dir_all(preferred).unwrap();
        fs::remove_dir_all(fallback).unwrap();
    }

    #[test]
    fn seals_a_closure_larger_than_the_external_manifest_limit_by_default() {
        // The count limit that bounds an externally supplied dependency manifest
        // does not bound a broker-resolved closure: a plug-in is not rejected for
        // needing a big runtime, since sealing is what makes it loadable at all.
        let install = temp_dir("unbounded");
        let count = crate::session_dependency_manifest::MAX_SESSION_DEPENDENCIES + 3;
        let names: Vec<String> = (0..count).map(|index| format!("dep{index}.dll")).collect();
        for name in &names {
            write_pe(&install, name, &[]);
        }
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        let plugin = write_pe(&install, "effect.aex", &borrowed);
        let roots = vec![install.clone()];

        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert_eq!(closure.dependencies().len(), count);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn a_ceiling_is_reported_before_the_image_that_crosses_it_is_read() {
        // An operator who capped the closure must be told they hit the cap, not
        // whatever the first image past it happens to be — so the ceiling is
        // applied before that image is even opened.
        let install = temp_dir("ceiling-order");
        write_pe(&install, "first.dll", &[]);
        write_pe(&install, "second.dll", &[]);
        fs::write(
            install.join("third.dll"),
            crate::test_pe::pe64_with_broken_import_directory(),
        )
        .unwrap();
        let plugin = write_pe(
            &install,
            "effect.aex",
            &["first.dll", "second.dll", "third.dll"],
        );
        let roots = vec![install.clone()];

        assert_eq!(
            resolve_dependency_closure(DependencyClosureRequest {
                max_dependencies: Some(2),
                ..DependencyClosureRequest::new(&plugin, &roots)
            })
            .unwrap_err()
            .to_string(),
            "dependency closure module limit exceeded"
        );
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn fails_closed_over_a_caller_supplied_ceiling() {
        let install = temp_dir("bounds");
        let names: Vec<String> = (0..4).map(|index| format!("dep{index}.dll")).collect();
        for name in &names {
            write_pe(&install, name, &[]);
        }
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        let plugin = write_pe(&install, "effect.aex", &borrowed);
        let roots = vec![install.clone()];

        let over_count = resolve_dependency_closure(DependencyClosureRequest {
            max_dependencies: Some(3),
            ..DependencyClosureRequest::new(&plugin, &roots)
        })
        .unwrap_err();
        assert_eq!(
            over_count.to_string(),
            "dependency closure module limit exceeded"
        );

        let over_bytes = resolve_dependency_closure(DependencyClosureRequest {
            max_total_bytes: Some(8),
            ..DependencyClosureRequest::new(&plugin, &roots)
        })
        .unwrap_err();
        assert_eq!(
            over_bytes.to_string(),
            "dependency closure byte limit exceeded"
        );
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn a_survey_reports_what_it_reached_past_an_unreadable_image() {
        // A survey measures; it never feeds a dispatch. Stopping at the first
        // image it cannot parse would hand a caller nothing where it could have
        // had most of the closure — and the caller that needs this most is the
        // one recording why a resolution failed.
        let install = temp_dir("tolerant");
        let plugin = write_pe(&install, "effect.aex", &["good.dll", "broken.dll"]);
        write_pe(&install, "good.dll", &[]);
        // A PE whose import directory points outside every section: the resolver
        // refuses it, the survey counts it and keeps the rest.
        fs::write(
            install.join("broken.dll"),
            crate::test_pe::pe64_with_broken_import_directory(),
        )
        .unwrap();

        let roots = vec![install.clone()];
        assert!(
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).is_err()
        );
        let survey = survey_dependency_closure(&plugin, &roots).unwrap();
        assert_eq!(survey.unreadable_images, 1);
        let mut reached: Vec<String> = survey
            .modules
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into())
            .collect();
        reached.sort();
        assert_eq!(reached, vec!["broken.dll", "good.dll"]);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn survey_measures_a_closure_without_paying_for_it() {
        let install = temp_dir("survey");
        let names: Vec<String> = (0..4).map(|index| format!("dep{index}.dll")).collect();
        for name in &names {
            write_pe(&install, name, &[]);
        }
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        let plugin = write_pe(&install, "effect.aex", &borrowed);
        let roots = vec![install.clone()];

        // A survey answers "how much would one dispatch copy" without hashing or
        // copying anything, including for a closure a caller chose to cap.
        assert!(
            resolve_dependency_closure(DependencyClosureRequest {
                max_dependencies: Some(2),
                ..DependencyClosureRequest::new(&plugin, &roots)
            })
            .is_err()
        );
        let survey = survey_dependency_closure(&plugin, &roots).unwrap();
        assert_eq!(survey.modules.len(), 4);
        assert!(survey.total_bytes > 0);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn rejects_unusable_search_roots() {
        let install = temp_dir("roots");
        let plugin = write_pe(&install, "effect.aex", &[]);
        let relative = vec![PathBuf::from("relative-root")];
        assert_eq!(
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &relative))
                .unwrap_err()
                .to_string(),
            "dependency search root must be absolute"
        );
        let too_many: Vec<PathBuf> = (0..=MAX_SEARCH_ROOTS).map(|_| install.clone()).collect();
        assert_eq!(
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &too_many))
                .unwrap_err()
                .to_string(),
            "dependency search root limit exceeded"
        );
        let file_root = vec![plugin.clone()];
        assert!(
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &file_root)).is_err()
        );
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn resolved_dependencies_carry_the_authenticated_identity() {
        let install = temp_dir("identity");
        let plugin = write_pe(&install, "effect.aex", &["runtime.dll"]);
        let dependency = write_pe(&install, "runtime.dll", &[]);
        let roots = vec![install.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        let bytes = fs::read(&dependency).unwrap();
        let expected: [u8; 32] = Sha256::digest(&bytes).into();
        assert_eq!(closure.dependencies()[0].expected_sha256, expected);
        assert_eq!(closure.dependencies()[0].expected_size, bytes.len() as u64);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn resolves_import_names_that_live_in_another_section() {
        // Descriptors in `.idata`, name strings in `.rdata`: a reader that binds
        // the name section from the first descriptor alone loses these, which
        // would drop dependencies from the closure without saying so. Two of the
        // 353 After Effects plug-ins measured for #304 are built this way.
        let install = temp_dir("sections");
        let plugin = install.join("effect.aex");
        fs::write(
            &plugin,
            crate::test_pe::pe64_with_names_in_a_second_section(&["first.dll", "second.dll"]),
        )
        .unwrap();
        write_pe(&install, "first.dll", &[]);
        write_pe(&install, "second.dll", &[]);

        let roots = vec![install.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        let mut sealed: Vec<String> = closure
            .dependencies()
            .iter()
            .map(|artifact| artifact.path.file_name().unwrap().to_string_lossy().into())
            .collect();
        sealed.sort();
        assert_eq!(sealed, vec!["first.dll", "second.dll"]);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn seals_delay_loaded_dependencies_too() {
        let install = temp_dir("delay");
        let plugin = install.join("effect.aex");
        fs::write(
            &plugin,
            crate::test_pe::pe64_with_imports(&["direct.dll"], &["delayed.dll"]),
        )
        .unwrap();
        write_pe(&install, "direct.dll", &[]);
        write_pe(&install, "delayed.dll", &[]);

        let roots = vec![install.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        let mut sealed: Vec<String> = closure
            .dependencies()
            .iter()
            .map(|artifact| artifact.path.file_name().unwrap().to_string_lossy().into())
            .collect();
        sealed.sort();
        assert_eq!(sealed, vec!["delayed.dll", "direct.dll"]);
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn fails_closed_on_an_import_table_over_the_per_image_ceiling() {
        let install = temp_dir("imports");
        let names: Vec<String> = (0..=MAX_IMPORT_NAMES_PER_IMAGE)
            .map(|index| format!("d{index}.dll"))
            .collect();
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        let plugin = write_pe(&install, "effect.aex", &borrowed);
        let roots = vec![install.clone()];
        assert_eq!(
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots))
                .unwrap_err()
                .to_string(),
            "imported name limit exceeded"
        );
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn fails_closed_on_an_import_name_with_no_terminator() {
        // The search for the terminator stops at the filename limit, so a
        // descriptor aimed at a long run of non-zero bytes costs a bounded scan
        // rather than the rest of the section — times every descriptor in the
        // image. The bound itself is not observable from here; what is, and what
        // this pins, is that such a name is still refused rather than truncated
        // into a plausible-looking dependency.
        let install = temp_dir("unterminated");
        let plugin = install.join("effect.aex");
        fs::write(
            &plugin,
            crate::test_pe::pe64_with_an_unterminated_import_name(),
        )
        .unwrap();
        let roots = vec![install.clone()];
        assert_eq!(
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots))
                .unwrap_err()
                .to_string(),
            "plug-in import name is unreadable"
        );
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn the_longest_name_a_filename_can_hold_is_still_read() {
        // The boundary the bounded scan introduces: the terminator of a
        // maximum-length name sits at the last searched byte. Off by one here and
        // every such import would silently become "unreadable".
        let install = temp_dir("longest");
        let longest = format!("{}.dll", "n".repeat(MAX_IMPORT_NAME_BYTES - ".dll".len()));
        let plugin = write_pe(&install, "effect.aex", &[&longest]);
        let roots = vec![install.clone()];

        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert_eq!(closure.unresolved(), [longest.to_lowercase()]);

        // One byte more is over the limit, as it was before the scan was bounded.
        let too_long = format!("n{longest}");
        let plugin = write_pe(&install, "over.aex", &[&too_long]);
        assert_eq!(
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots))
                .unwrap_err()
                .to_string(),
            "plug-in import name is unreadable"
        );
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn non_pe_content_contributes_no_imports() {
        let install = temp_dir("nonpe");
        let plugin = install.join("effect.aex");
        fs::write(&plugin, b"this is not a PE image").unwrap();
        let roots = vec![install.clone()];
        let closure =
            resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
        assert!(closure.is_empty());
        assert!(closure.unresolved().is_empty());
        fs::remove_dir_all(install).unwrap();
    }
}
