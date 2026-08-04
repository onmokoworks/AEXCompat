/// Plugin configuration, from the TOML config file (see [`config_path`]). Env
/// vars override `dir`/`repository`. Absent fields fall back to those env vars.
#[derive(Default, serde::Deserialize)]
struct Config {
    /// A single folder scanned for `*.aex` (backward-compatible; merged with
    /// `dirs`). Kept so existing single-folder configs keep working.
    dir: Option<PathBuf>,
    /// Folders scanned recursively for `*.aex`. When neither this, `dir`, nor the
    /// env override is set, the default After Effects / MediaCore plug-in folders
    /// are used (see [`default_dirs`]).
    #[serde(default)]
    dirs: Vec<PathBuf>,
    /// Repo root holding the built workers.
    repository: Option<PathBuf>,
    /// Folders searched for an AEX's dependency DLLs (issue #304). The AEX's own
    /// folder is always searched first; these are the extra runtime folders an
    /// installed host would have provided (for an AE effect, the AE
    /// `Support Files\` folder). Empty means the default AE runtime folders.
    #[serde(default)]
    dependency_dirs: Vec<PathBuf>,
    /// Optional ceiling on how many DLLs may be sealed with one AEX. Absent means
    /// no ceiling: the closure is whatever the plug-in imports out of the folders
    /// above, and a plug-in is not skipped for needing a large runtime. Set it to
    /// trade coverage for a shorter discovery pass — an AEX over the ceiling then
    /// fails discovery instead of copying its closure.
    dependency_module_limit: Option<usize>,
    /// Optional ceiling on the total bytes sealed with one AEX, same trade-off as
    /// `dependency_module_limit`. The heaviest AE plug-ins pull about 1 GB.
    dependency_byte_limit: Option<u64>,
    /// Effect names to skip (matched against each AEX's file stem, case- and
    /// `.aex`-extension-insensitive).
    #[serde(default)]
    ignore: Vec<String>,
}

/// The TOML config path: `AEXCOMPAT_MULTIFILTER_CONFIG` if set, else the Windows
/// per-user default `%APPDATA%\aexcompat-multifilter\config.toml`.
fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_CONFIG) {
        return Some(PathBuf::from(path));
    }
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("aexcompat-multifilter")
            .join("config.toml"),
    )
}

/// Loads the config file, or a default (all-absent) config when it is missing or
/// malformed — the plug-in then relies on the env vars, or registers nothing.
fn load_config() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Having no config file is the normal case and says nothing. Having one
        // that cannot be read drops every setting in it just like a parse error
        // does, and misdirects the same way (issue #655).
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Config::default();
        }
        Err(error) => {
            log_warn(&format!(
                "{} could not be read, so every setting in it is ignored: {error}",
                path.display()
            ));
            return Config::default();
        }
    };
    match toml::from_str(&text) {
        Ok(config) => config,
        Err(error) => {
            // Silently defaulting drops `repository` along with everything else,
            // and the "no worker found" warning then tells a user who *did* set
            // `repository` to go set it. Name the parse error instead (issue #655).
            log_warn(&format!(
                "{} could not be parsed, so every setting in it is ignored: {error}",
                path.display()
            ));
            Config::default()
        }
    }
}

/// Drops a trailing `.aex` extension (any case) from an ignore entry so it may
/// be written with or without it. Checks the last four bytes case-insensitively,
/// then slices the `str` only on that ASCII-`.aex` match — where the byte at
/// `len - 4` is `.` and therefore a char boundary — so a multi-byte entry never
/// slices mid-character (that would panic across the `extern "C"` boundary).
fn strip_aex_ext(entry: &str) -> &str {
    let bytes = entry.as_bytes();
    if bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".aex") {
        &entry[..entry.len() - 4]
    } else {
        entry
    }
}

/// Whether `path`'s file stem matches an ignore entry (case-insensitive; an
/// entry may be written with or without the `.aex` extension).
fn is_ignored(path: &Path, ignore: &[String]) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    ignore
        .iter()
        .any(|entry| strip_aex_ext(entry).eq_ignore_ascii_case(stem))
}

/// Cap on concurrent discovery workers. Each spawns an L2 worker subprocess that
/// loads the AEX + the compat runtime (memory-heavy). Kept low: too much
/// concurrency causes resource contention that pushes a plain ~2 s discovery past
/// the worker's 5 s deadline, so a discoverable effect times out and is wrongly
/// cached as a non-effect. Discovery runs on a background thread, so a low cap is
/// cheap. (Measured: 8-way ≈ 68% false timeouts, serial ≈ 3%.)
const MAX_DISCOVERY_PARALLELISM: usize = 3;
/// Recursion depth cap for the folder scan. A backstop against a pathological
/// tree eating the stack, not the cycle guard — `visited` holds canonical paths
/// and already breaks link loops.
///
/// It was 8 on the assumption that "the AE plug-in tree is only a few levels
/// deep". A real After Effects 2026 install goes to 10:
/// `Plug-ins\Effects\mochaAE\Resources\mochaui\qml\QtQuick\Dialogs\quickimpl\qml\+Fusion`
/// (Qt resources, no `.aex` below). Nothing was lost, but every launch hit the
/// cap and so reported a non-authoritative scan forever, which permanently
/// suppressed the prune (issue #660). Sized well clear of any real install.
const MAX_SCAN_DEPTH: usize = 32;

/// The background discovery saves the cache after each chunk of this many AEX, so
/// progress survives a restart/shutdown mid-scan (rather than only at the end of a
/// multi-minute scan).
const DISCOVERY_SAVE_CHUNK: usize = 24;

/// Set on plugin unload so the background discovery thread stops promptly.
static DISCOVERY_SHUTDOWN: AtomicBool = AtomicBool::new(false);
/// The background discovery thread's join handle, so `UninitializePlugin` can
/// stop and join it (bounded by one in-flight worker deadline).
static DISCOVERY_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "C" fn RegisterPlugin(host: *mut HOST_APP_TABLE) {
    if host.is_null() {
        log_warn("the host passed no registration table; no AEX filter is registered");
        return;
    }
    let config = load_config();
    let Some((repository, root_source)) = resolve_worker_root(
        std::env::var_os(ENV_REPOSITORY).map(PathBuf::from),
        config.repository.as_deref(),
        self_module_path(),
    ) else {
        log_warn(&missing_worker_advice());
        return;
    };
    let (dirs, dirs_complete) = resolve_scan_dirs(&config);
    // Said before the scan, so every early return below still leaves the log
    // showing which worker root and which folders this launch used.
    report_worker_root(&repository, root_source, &dirs);

    // Recursively collect the *.aex to expose (minus ignored), deduped + sorted.
    // A non-authoritative scan makes the background pass keep (rather than prune)
    // the entries it did not see this launch. Each cause is carried separately
    // because they need different fixes from the user (issue #660).
    let scan = collect_aex(&dirs, &config.ignore);
    let mut limits = scan.limits;
    limits.unresolved_root |= !dirs_complete;
    let scan_complete = limits.authoritative();
    if limits.too_deep {
        log_warn(&depth_cap_warning());
    }

    // Discovery (an L2 worker per AEX) is slow — hundreds of AE effects take
    // minutes — and can only ever populate the cache, since AviUtl2 freezes a
    // filter's config at load and cannot register a filter discovered later. So
    // register from the cache immediately (never blocking startup) and discover
    // the rest on a background thread whose results appear on the NEXT launch.
    // The cache is keyed by AEX (path, mtime, len), but a discovery *result* also
    // depends on the compat host that produced it (the L2 worker and this DLL's
    // in-process broker), so an entry made by an older host is re-verified in the
    // background (issue #304). It is NOT dropped: an unregistered filter makes
    // AviUtl2 drop every object referencing it when a saved project is opened, and
    // saving then deletes those objects for good, so a host rebuild must never
    // empty the filter list for a launch (issue #307).
    let dependency = resolve_dependency_config(&config);
    let build = build_fingerprint(&repository, &dependency);
    let mut cache = load_cache();
    let mut plugins = scan.plugins;
    // A scan that cannot be trusted must not make a cached effect disappear
    // for this launch. Registration is intentionally conservative in the
    // other direction: a temporarily missing AEX may be shown and fail closed
    // at render time, but AviUtl2 will not discard objects from a saved project
    // merely because this launch could not see the file (#321).
    plugins.extend(cached_fallback_plugins(
        &cache,
        &scan.seen,
        &dirs,
        scan_complete,
        !dirs_complete,
        &config.ignore,
    ));
    plugins.sort();
    plugins.dedup();
    if plugins.is_empty() {
        // Also an empty filter list, and also worth saying out loud: a mistyped
        // `dir`, an AE install that moved, or an over-broad `ignore` reaches here
        // rather than any of the failure paths below (issue #655).
        log_warn(&empty_scan_summary(&dirs, scan.seen.len(), limits));
        return;
    }

    // Register (host callback, main thread only) each AEX whose discovery already
    // succeeded. A changed AEX keeps its last known-good registration for this
    // launch while the replacement is discovered in the background; an
    // unregistered filter would let AviUtl2 discard objects from saved projects.
    // Unknown entries and old-host entries also go to the background pass; its
    // updated result is picked up on the next launch.
    // Computed over the whole set before any registration: a name can only be
    // known to collide once every plug-in is known (issue #661). `plugins` is
    // sorted and deduped above, so the assignment does not depend on scan order.
    // Everything the cache still remembers counts too, so a folder that could not
    // be read this launch does not rename a filter that did register. This
    // narrows the hazard rather than closing it: a peer the cache has never seen
    // (a first launch that misses a folder) still cannot be counted.
    let filter_names = unique_filter_names(
        &plugins,
        &cached_naming_peers(&cache, &dirs, scan_complete, !dirs_complete, &config.ignore),
    );
    report_qualified_names(&plugins, &filter_names);
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut registered: usize = 0;
    let mut aliases: Option<HashMap<PathBuf, Vec<String>>> = None;
    let mut rekey: Vec<(String, String)> = Vec::new();
    // Whether any cached key under the scan roots is a spelling this scan did not
    // walk. If none is, no other spelling exists and the alias lookup — which
    // touches the filesystem, on the thread AviUtl2 is loading from — is skipped.
    let walked: std::collections::HashSet<String> = scan
        .seen
        .iter()
        .map(|plugin| plugin.to_string_lossy().into_owned())
        .collect();
    let alias_possible = alias_possible(&cache, &walked, &dirs);

    for (plugin, filter_name) in plugins.iter().zip(&filter_names) {
        let key = plugin.to_string_lossy().into_owned();
        let meta = file_meta(plugin);
        let (cached, alias) = resolve_cached(
            &cache,
            &key,
            plugin,
            meta,
            build,
            &dirs,
            alias_possible,
            &mut aliases,
        );
        if let Some(alias) = alias {
            rekey.push((alias, key));
        }
        let mut decision = classify(cached, meta, build);
        // A closure that would now resolve differently (issue #304) joins the same
        // queue rather than unregistering the filter: the dependency DLLs decide
        // the result as much as the host build does, and #307's rule is that
        // nothing is unregistered for a launch.
        //
        // Bounded by the same [`RETRY_BUDGET`] a host change is. A re-discovery
        // that keeps failing does not update the recorded closure (`keep_best`
        // refuses to demote a working entry, and keeps its record with it), so the
        // trigger would otherwise fire on every launch forever — an AE update that
        // rewrites one runtime DLL puts every effect in that state at once.
        // Checked only when the entry would otherwise be left alone, so an
        // already-queued one pays nothing.
        if !decision.discover
            && let Some(entry) = cached
            && needs_closure_recheck(entry, build, &search_roots_for(plugin, &dependency.dirs))
        {
            decision.discover = true;
        }
        if decision.register
            && let Some(entry) = cached
        {
            register_discovered(host, &repository, plugin, &dependency, entry, filter_name);
            registered += 1;
        }
        if decision.discover {
            pending.push(plugin.clone());
        }
    }

    // Counted over `plugins` — the set actually iterated above — not over
    // `scan.seen`: an untrustworthy scan adds cached entries that were not walked
    // this launch (#321), which would otherwise read as "registered 500 of 12".
    report_registration(plugins.len(), registered, pending.len(), limits);

    let rekeyed = !rekey.is_empty();
    apply_rekey(&mut cache, rekey);

    if pending.is_empty() {
        // Nothing to discover, so the background pass (the only other writer)
        // will not run. Persist the re-key here or it is recomputed every launch.
        if rekeyed && !save_cache(&cache) {
            log_warn(
                "the discovery cache could not be written; the plug-in paths it \
                 re-keyed this launch are resolved again on the next one",
            );
        }
        return;
    }

    spawn_background_discovery(
        repository,
        dependency,
        scan.seen,
        dirs,
        cache,
        pending,
        build,
        scan_complete,
    );
}

/// Drops cache entries for AEX that are no longer present.
///
/// An entry may only be judged gone if this launch actually looked where it
/// lives, and looked completely. Two guards, because "not in this scan" is not
/// "deleted", and dropping a live entry leaves that effect unregistered on the
/// next launch, deleting objects from saved projects that use it (issue #307):
///
/// - `scan_complete` is false for any of [`ScanLimits`]: a default folder that
///   would not resolve, a folder that could not be read, a tree past the depth
///   cap. The causes differ for the user; for the prune they are one answer.
/// - `roots` bounds the prune to the folders scanned. The cache file is shared
///   across configurations, so pointing `AEXCOMPAT_MULTIFILTER_DIR` at one folder
///   for a launch would otherwise delete every entry from the default AE and
///   MediaCore folders, and the next unset launch would register none of them.
/// - `seen` is every AEX found, `ignore`d ones included, since those exist on
///   disk; judging from the registered subset would drop an ignored effect's
///   entry and leave it unregistered the launch after it is un-ignored.
/// - Anything still missing is confirmed against the filesystem before it goes,
///   so a path the scan reached under a different spelling (through a junction)
///   is not mistaken for a deleted one.
///
/// Keeping a stale entry costs only cache bytes. An incomplete scan also uses
/// the cache as a registration fallback, so an effect can remain visible while
/// its folder is temporarily unavailable.
fn prune_cache(
    cache: &mut HashMap<String, CacheEntry>,
    seen: &[PathBuf],
    roots: &[PathBuf],
    scan_complete: bool,
) {
    if !scan_complete {
        return;
    }
    // Compare presence on the same lossy string the cache is keyed by, so a path
    // that does not round-trip through UTF-8 still matches itself; `starts_with`
    // needs a Path, but only decides whether this launch looked there at all.
    let present: std::collections::HashSet<String> = seen
        .iter()
        .map(|plugin| plugin.to_string_lossy().into_owned())
        .collect();
    cache.retain(|key, _| {
        let path = Path::new(key);
        if present.contains(key) || !roots.iter().any(|root| path.starts_with(root)) {
            return true;
        }
        // Missing from this launch's listing is not the same as gone: a junction
        // can make one AEX reachable under several paths and the scan keeps only
        // the spelling it walked. Ask the filesystem instead, and keep the entry
        // unless it answers a definite "no" — an error is "could not tell". (A
        // path behind an unresolvable link answers `Ok(false)`, not an error; that
        // case is held off by `scan_complete`, which is false for such a link.)
        !matches!(path.try_exists(), Ok(false))
    });
}

/// The AEX an *untrustworthy* scan did not list but the cache still remembers,
/// under the scan roots, ignored ones excluded. Used only to count filter-name
/// collisions (issue #661): a plug-in this launch could not see must still hold
/// its claim on a name, or the one that did register gets renamed and saved
/// projects lose its objects (issue #307).
///
/// Empty for a complete scan, the same authority rule [`cached_fallback_plugins`]
/// follows. A complete scan already listed every plug-in that can register, so
/// the cache could only contribute names for files that are gone — and
/// `prune_cache` runs only when the background pass does, so a deleted plug-in's
/// key survives every launch that has nothing to discover. Counting it would
/// qualify a name with no rival left, renaming a filter saved projects refer to.
///
/// Unlike [`cached_fallback_plugins`] this does not filter on `entry.ok`. A
/// plug-in that has never discovered successfully is not registered, but it is
/// still on disk and takes the name as soon as it does discover, so its claim has
/// to stand either way.
///
/// A re-keyed entry is deliberately kept under both spellings ([`apply_rekey`]),
/// and counting one file twice would invent a collision just the same. The
/// retained spelling marks itself, so drop it while its walked spelling is still
/// cached. A *moved* plug-in leaves a second key that carries no such mark and no
/// filesystem check may be spent finding out (this runs on the load thread, see
/// [`alias_possible`]), so it can still over-count until a prune — bounded to
/// launches whose scan was already untrustworthy.
fn cached_naming_peers(
    cache: &HashMap<String, CacheEntry>,
    roots: &[PathBuf],
    scan_complete: bool,
    roots_incomplete: bool,
    ignore: &[String],
) -> Vec<PathBuf> {
    if scan_complete {
        return Vec::new();
    }
    cache
        .iter()
        .filter(|(_, entry)| {
            !entry.alias_fallback
                || !entry
                    .alias_target
                    .as_ref()
                    .is_some_and(|target| cache.contains_key(target))
        })
        .map(|(key, _)| PathBuf::from(key))
        .filter(|path| {
            // When a *default* root could not be resolved this launch, the roots
            // no longer describe where the plug-ins are; filtering on them would
            // drop exactly the peers this function exists to keep. Same reasoning
            // as `cached_fallback_plugins`.
            roots_incomplete || roots.is_empty() || roots.iter().any(|root| path.starts_with(root))
        })
        .filter(|path| !is_ignored(path, ignore))
        .collect()
}

/// Says which filters had to be renamed to avoid a collision. A qualified name is
/// user-visible and, per issue #662, changes how saved projects resolve the
/// filter, so it should not happen silently.
fn report_qualified_names(plugins: &[PathBuf], names: &[String]) {
    let line = qualified_names_summary(plugins, names);
    if !line.is_empty() {
        log_info(&line);
    }
}

/// The renamed filters as one line, or empty when nothing was renamed. Split out
/// so the wording is testable without a host log handle.
fn qualified_names_summary(plugins: &[PathBuf], names: &[String]) -> String {
    // Paired with the path, because the name alone cannot say which file took it:
    // a numeric fallback yields `Threshold (Effects)` and `Threshold (Effects) [2]`
    // for two files whose folders are spelled the same.
    let qualified: Vec<String> = plugins
        .iter()
        .zip(names)
        .filter(|(plugin, name)| name.as_str() != filter_stem(plugin))
        .map(|(plugin, name)| format!("{name} <- {}", plugin.display()))
        .collect();
    if qualified.is_empty() {
        return String::new();
    }
    // Bounded: the list identifies the collision, it does not enumerate a
    // pathological install.
    const LISTED: usize = 8;
    let rest = qualified.len().saturating_sub(LISTED);
    format!(
        "{} filter name(s) qualified to avoid a duplicate: {}{}",
        qualified.len(),
        qualified
            .iter()
            .take(LISTED)
            .map(String::as_str)
            .collect::<Vec<&str>>()
            .join("; "),
        if rest > 0 {
            format!("; and {rest} more")
        } else {
            String::new()
        }
    )
}

/// Returns usable cached AEX paths that a non-authoritative scan did not see.
///
/// A complete scan is authoritative: resurrecting an entry absent from it
/// would keep filters for files that really disappeared. An incomplete scan is
/// the opposite: absence is not evidence of deletion, so keeping a last-known-
/// good registration is safer than letting AviUtl2 remove project objects
/// before the file becomes visible again (#321).
fn cached_fallback_plugins(
    cache: &HashMap<String, CacheEntry>,
    seen: &[PathBuf],
    roots: &[PathBuf],
    scan_complete: bool,
    roots_incomplete: bool,
    ignore: &[String],
) -> Vec<PathBuf> {
    if scan_complete {
        return Vec::new();
    }
    let seen_keys: HashSet<String> = seen
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let seen_real: HashSet<PathBuf> = seen
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect();

    cache
        .iter()
        .filter_map(|(key, entry)| {
            if !entry.ok || seen_keys.contains(key) {
                return None;
            }
            let path = PathBuf::from(key);
            if !roots_incomplete
                && !roots.is_empty()
                && !roots.iter().any(|root| path.starts_with(root))
            {
                return None;
            }
            if is_ignored(&path, ignore)
                || path
                    .canonicalize()
                    .is_ok_and(|real| seen_real.contains(&real))
            {
                return None;
            }
            Some(path)
        })
        .collect()
}

/// Indexes the cache by each entry's real (link-resolved) path, so an AEX whose
/// walked spelling changed between launches is still found. Keys that no longer
/// resolve are skipped; they are handled by [`prune_cache`].
///
/// Restricted to `roots` as a trade-off, not because keys outside them cannot
/// match: resolving one could match too, but a leftover key on a disconnected
/// drive would stall startup. So a scan root whose own spelling changed between
/// launches (its path is now written a different way) is not resolved, and its
/// effects go unregistered for that launch — the case tracked as #321.
fn index_by_real_path(
    cache: &HashMap<String, CacheEntry>,
    roots: &[PathBuf],
    build: BuildFingerprint,
) -> HashMap<PathBuf, Vec<String>> {
    let mut index: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for key in cache.keys().filter(|key| {
        roots
            .iter()
            .any(|root| Path::new(key.as_str()).starts_with(root))
    }) {
        let Ok(real) = Path::new(key).canonicalize() else {
            continue;
        };
        index.entry(real).or_default().push(key.clone());
    }
    // Best first, and every candidate kept: ranking cannot tell whether an entry
    // still describes the file on disk, so the caller has to be able to fall
    // through to the next spelling rather than be handed one unusable pick and
    // leave the effect unregistered (issue #307). The key breaks the remaining
    // tie, so the order never depends on hash iteration order — which would make
    // the effect's parameters, or whether it registers at all, differ between
    // launches.
    for keys in index.values_mut() {
        keys.sort_by(|left, right| {
            alias_rank(cache.get(right), build)
                .cmp(&alias_rank(cache.get(left), build))
                .then_with(|| left.cmp(right))
        });
    }
    index
}

/// Whether any cached key under `roots` is a spelling this scan did not walk
/// and is still worth resolving.
///
/// If none is, every cached entry in scope is already keyed by the path the scan
/// produced, so no other spelling exists to look for and the alias lookup — which
/// canonicalizes paths on the thread AviUtl2 is loading from — can be skipped.
fn alias_possible(
    cache: &HashMap<String, CacheEntry>,
    walked: &std::collections::HashSet<String>,
    roots: &[PathBuf],
) -> bool {
    cache.iter().any(|(key, entry)| {
        !walked.contains(key)
            && roots
                .iter()
                .any(|root| Path::new(key.as_str()).starts_with(root))
            && (!entry.alias_fallback
                || entry
                    .alias_target
                    .as_deref()
                    .is_none_or(|target| !cache.contains_key(target)))
    })
}

/// Picks the cache entry to use for one AEX, and the alias key it came from when
/// that was not the spelling this scan walked.
///
/// Entries are keyed by the path string the scan walked, and a junction added,
/// renamed, or reached from another root changes that spelling without changing
/// the file. The file is looked for under another spelling whenever what is held
/// under this one would not register, or the effect goes unregistered for the
/// launch — which deletes it out of saved projects that use it (issue #307). Not
/// only on an outright miss: only the walked spelling is refreshed by discovery,
/// so a copy left under another one can be the newer of the two. Also when this
/// spelling holds a `stale` entry, which registers but on a payload that may
/// describe older bytes, so a sound copy elsewhere is worth preferring.
#[allow(clippy::too_many_arguments)]
fn resolve_cached<'a>(
    cache: &'a HashMap<String, CacheEntry>,
    key: &str,
    plugin: &Path,
    meta: Option<((u64, u32), u64)>,
    build: BuildFingerprint,
    roots: &[PathBuf],
    alias_possible: bool,
    aliases: &mut Option<HashMap<PathBuf, Vec<String>>>,
) -> (Option<&'a CacheEntry>, Option<String>) {
    let direct = cache.get(key);
    let direct_registers = classify(direct, meta, build).register;
    let direct_matches = direct.is_some_and(|entry| {
        meta.is_some_and(|(mtime, len)| entry.mtime == mtime && entry.len == len)
    });
    // Look further when this spelling holds nothing usable, and also when what it
    // holds is only usable in the weaker sense of `alias_rank` — a stale entry
    // registers, but on a payload that may describe older bytes, so its sessions
    // fail to open and its frames pass through unrendered. Another spelling can
    // hold a sound entry for the same file.
    let direct_is_sound =
        direct_registers && direct_matches && direct.is_some_and(|entry| !entry.stale);
    if !alias_possible || direct_is_sound {
        return (direct, None);
    }
    // Built lazily, so a launch where every spelling matches never pays for it.
    let index = aliases.get_or_insert_with(|| index_by_real_path(cache, roots, build));
    let Some(candidates) = plugin.canonicalize().ok().and_then(|real| index.get(&real)) else {
        return (direct, None);
    };
    // Best-ranked first, but try each: the rank cannot tell whether an entry still
    // describes the file, so a better-ranked but outdated one must not shadow a
    // usable one and leave the effect unregistered.
    for alias in candidates {
        let candidate = cache.get(alias);
        let candidate_matches = candidate.is_some_and(|entry| {
            meta.is_some_and(|(mtime, len)| entry.mtime == mtime && entry.len == len)
        });
        // Take it if this spelling had nothing usable, or if the candidate is
        // strictly sounder — never a lateral move, which would just churn.
        let improves = !direct_registers
            || !direct_matches
            || alias_rank(candidate, build) > alias_rank(direct, build);
        // A changed direct hit is intentionally retained for this launch, but
        // an outdated alias must not shadow a spelling whose metadata matches
        // the file currently being loaded.
        if improves && candidate_matches && classify(candidate, meta, build).register {
            return (candidate, Some(alias.clone()));
        }
    }
    (direct, None)
}

/// Ranks one spelling of a file against another as the entry to reuse. Two
/// spellings can disagree because only the walked one is refreshed by discovery:
/// prefer the one that registers, then one whose parameters are not known to be
/// out of date, then the one the current host produced.
///
/// An unknown current build matches nothing rather than everything: it equals
/// `BuildFingerprint::default()`, which is also what an entry written before the
/// field existed carries, so comparing would rank a legacy entry above a freshly
/// discovered one. Same reasoning as `classify`'s `is_known` guard.
fn alias_rank(entry: Option<&CacheEntry>, build: BuildFingerprint) -> (bool, bool, bool) {
    match entry {
        Some(entry) => (
            entry.ok,
            !entry.stale,
            build.is_known() && entry.build == build,
        ),
        None => (false, false, false),
    }
}

/// Copies each aliased entry onto the spelling the scan actually walked.
///
/// The background pass keys by that spelling, so without this it would find no
/// cached entry and `keep_best`'s refusal to demote would never apply — a
/// transient discovery failure could then write a negative and unregister the
/// effect on the next launch (issue #307).
///
/// The alias is copied, not moved: the walked spelling may be the temporary one.
/// If the scan reached the AEX through a junction that is gone next launch, the
/// walked key no longer resolves, and having deleted the original would leave
/// nothing to find. Keeping both costs one entry until [`prune_cache`] sees a
/// spelling genuinely stop existing, which is the safe direction here.
fn apply_rekey(cache: &mut HashMap<String, CacheEntry>, rekey: Vec<(String, String)>) {
    for (alias, walked) in rekey {
        if let Some(entry) = cache.get(&alias).cloned() {
            let mut walked_entry = entry.clone();
            walked_entry.alias_fallback = false;
            walked_entry.alias_target = None;
            cache.insert(walked.clone(), walked_entry);
            if alias != walked {
                if let Some(alias_entry) = cache.get_mut(&alias) {
                    alias_entry.alias_fallback = true;
                    alias_entry.alias_target = Some(walked);
                }
            }
        }
    }
}

/// What to do with one AEX at load.
#[derive(PartialEq, Eq, Debug)]
struct LoadDecision {
    /// Register it now from the cached parameters.
    register: bool,
    /// (Re-)discover it on the background thread.
    discover: bool,
}

/// Decides both from the cached entry, the AEX's current `(mtime, len)`, and the
/// current host build.
///
/// The two are independent on purpose: an entry discovered by an older host is
/// still registered while it is re-verified, because not registering it would let
/// AviUtl2 delete every object that uses it out of a saved project (issue #307).
///
/// `meta` is `None` when the AEX could not be stat'd even though the scan just
/// found the path (a sharing violation, a deploy race between `read_dir` and
/// `metadata`). As in [`keep_best`], that is not evidence the file changed, so the
/// cached result keeps being registered — a failed stat must not be able to
/// unregister a filter for a launch. Re-discovery is queued either way, since
/// freshness could not be confirmed.
fn classify(
    cached: Option<&CacheEntry>,
    meta: Option<((u64, u32), u64)>,
    build: BuildFingerprint,
) -> LoadDecision {
    let Some(entry) = cached else {
        return LoadDecision {
            register: false,
            discover: true,
        };
    };
    match meta {
        // Confirmed unchanged: re-verify when the entry is marked stale, or when
        // the host build moved (but an unknown build is not a moved one — see
        // `BuildFingerprint::is_known`, or one failed stat costs two full passes)
        // and this host has not already spent its [`RETRY_BUDGET`] on it.
        Some((mtime, len)) if entry.mtime == mtime && entry.len == len => LoadDecision {
            register: entry.ok,
            discover: entry.stale
                || (build.is_known()
                    && entry.build != build
                    && (entry.checked != build || entry.attempts < RETRY_BUDGET)),
        },
        // Confirmed changed: keep a last-known-good effect registered for this
        // launch and discover the replacement in the background. The cached
        // sha/params may be incompatible with the new bytes, but RenderSession
        // then fails closed on the SHA instead of AviUtl2 dropping the object
        // before the replacement result is available (issue #309).
        Some(_) => LoadDecision {
            register: entry.ok,
            discover: true,
        },
        // Unknown: keep what we have and re-check in the background.
        None => LoadDecision {
            register: entry.ok,
            discover: true,
        },
    }
}

/// Discovers the pending AEX on a background thread and rewrites the cache, so
/// startup is never blocked. Newly-discovered effects appear on the next launch.
fn spawn_background_discovery(
    repository: PathBuf,
    dependency: DependencyConfig,
    // Every AEX the scan saw, ignored ones included: the prune judges existence
    // from this, not from the registered subset (issue #307).
    seen: Vec<PathBuf>,
    roots: Vec<PathBuf>,
    mut cache: HashMap<String, CacheEntry>,
    pending: Vec<PathBuf>,
    build: BuildFingerprint,
    scan_complete: bool,
) {
    // Said before the spawn, so it is never a promise a failed spawn leaves
    // unkept: `report_registration` has already told the user these are queued.
    log_info(&format!(
        "discovering {} plug-in(s) in the background; results appear on the next \
         launch",
        pending.len()
    ));
    let handle = std::thread::Builder::new()
        .name("aex-multifilter-discovery".into())
        .spawn(move || {
            // The pass runs arbitrary third-party AEX through the broker. A panic
            // here would end the thread silently, leaving the "results appear on
            // the next launch" line above as a promise nothing retracts — the
            // silence this issue exists to remove. `discover_all` already catches
            // per-plug-in panics; this covers the rest of the pass.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                run_discovery_pass(
                    &repository,
                    &dependency,
                    &seen,
                    &roots,
                    &mut cache,
                    &pending,
                    build,
                    scan_complete,
                )
            }));
            if outcome.is_err() {
                log_warn(
                    "background discovery stopped on an internal error; the \
                     plug-ins it had not reached stay undiscovered until the next \
                     launch",
                );
            }
        });
    match handle {
        Ok(handle) => {
            if let Ok(mut slot) = DISCOVERY_THREAD.lock() {
                // A previous launch's thread cannot exist (RegisterPlugin runs
                // once), so just store this one for UninitializePlugin to join.
                *slot = Some(handle);
            }
        }
        // Nothing will ever discover them, so the line above is now a promise
        // that cannot be kept; retract it rather than leave the user waiting for
        // filters that appear on no later launch either.
        Err(error) => log_warn(&format!(
            "could not start the background discovery thread ({error}); no new \
             plug-in will be discovered this launch"
        )),
    }
}

/// The background pass itself: prune, then discover in chunks, then report.
#[allow(clippy::too_many_arguments)]
fn run_discovery_pass(
    repository: &Path,
    dependency: &DependencyConfig,
    seen: &[PathBuf],
    roots: &[PathBuf],
    cache: &mut HashMap<String, CacheEntry>,
    pending: &[PathBuf],
    build: BuildFingerprint,
    scan_complete: bool,
) {
    // Prune stale entries (removed/renamed AEX) up front so an early shutdown
    // still leaves a pruned cache.
    prune_cache(cache, seen, roots, scan_complete);

    // Discover in chunks and save the cache after each, so a restart or shutdown
    // mid-scan keeps the progress so far (effects appear across successive
    // launches) instead of discarding a multi-minute scan. The full scan of
    // hundreds of AE effects can only ever populate the cache — AviUtl2 freezes a
    // filter's config at load — so the results show on the next launch.
    // Each entry carries the build that produced it, so an interrupted pass
    // leaves the not-yet-redone entries on the old build and they are queued
    // again next launch (issue #307).
    let (mut effects, mut rejected) = (0usize, 0usize);
    let mut interrupted = false;
    let mut persisted = true;
    for chunk in pending.chunks(DISCOVERY_SAVE_CHUNK) {
        if DISCOVERY_SHUTDOWN.load(Ordering::Relaxed) {
            interrupted = true;
            break;
        }
        let results = discover_all(repository, chunk, dependency, build);
        let discovered = results.len();
        for (plugin, entry) in results {
            if entry.ok {
                effects += 1;
            } else {
                rejected += 1;
            }
            let key = plugin.to_string_lossy().into_owned();
            // `None` means there was nothing trustworthy to write; the existing
            // entry keeps registering and is retried next launch.
            if let Some(merged) = keep_best(cache.get(&key), entry, file_meta(&plugin)) {
                cache.insert(key, merged);
            }
        }
        // discover_all returns fewer than the chunk only if it was cut short by
        // the shutdown flag; save what we have and stop.
        // Assigned, not accumulated: every save writes the whole map, so a chunk
        // that lost the cache lock is fully made up for by the next successful
        // write. Sticking on the earlier failure would warn that a pass "will not
        // survive the restart" when all of it did.
        persisted = save_cache(cache);
        if discovered < chunk.len() {
            interrupted = true;
            break;
        }
    }
    report_discovery(effects, rejected, interrupted, persisted);
}

/// The folders to scan: the env override wins, else `dir` + `dirs` from the
/// config, else the default After Effects / MediaCore plug-in folders.
///
/// The second value is false when a *default* folder could not be resolved this
/// launch (an AE update in progress, a drive not yet mounted). Explicitly
/// configured folders are always "complete": the user named them, so a missing
/// one is their intent, not a probe that failed. See [`collect_aex`] — an
/// incomplete resolution must not let the background pass prune that folder's
/// cache entries, which would unregister hundreds of effects (issue #307).
fn resolve_scan_dirs(config: &Config) -> (Vec<PathBuf>, bool) {
    if let Some(dir) = std::env::var_os(ENV_DIR) {
        return (vec![PathBuf::from(dir)], true);
    }
    let mut dirs: Vec<PathBuf> = config.dir.clone().into_iter().collect();
    dirs.extend(config.dirs.iter().cloned());
    if dirs.is_empty() {
        return default_dirs();
    }
    (dirs, true)
}

/// The default scan folders: the latest installed After Effects `Plug-ins`
/// folder and the shared Adobe MediaCore folder. Only existing paths are kept,
/// and the second value is false if either one could not be resolved, so a
/// transiently invisible AE install is not mistaken for "these effects are gone".
fn default_dirs() -> (Vec<PathBuf>, bool) {
    let (after_effects, ae_complete) = latest_after_effects_plugins();
    let (mediacore, mediacore_complete) = mediacore_dir();
    let complete =
        ae_complete && mediacore_complete && after_effects.is_some() && mediacore.is_some();
    (
        after_effects.into_iter().chain(mediacore).collect(),
        complete,
    )
}

/// How dependency closures are resolved for this launch (issue #304): the extra
/// folders to search — the env override wins, else the config, else the default
/// After Effects runtime folder — plus the operator's optional ceilings. An AEX's
/// own folder is not listed; it is always searched first, per plug-in.
fn resolve_dependency_config(config: &Config) -> DependencyConfig {
    let dirs = if let Some(dirs) = std::env::var_os(ENV_DEPENDENCY_DIRS) {
        dirs.to_string_lossy()
            .split(';')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .collect()
    } else if !config.dependency_dirs.is_empty() {
        config.dependency_dirs.clone()
    } else {
        default_dependency_dirs()
    };
    DependencyConfig {
        dirs,
        module_limit: config.dependency_module_limit,
        byte_limit: config.dependency_byte_limit,
    }
}

/// Where an AEX's dependency DLLs are looked for, and the optional ceilings on
/// what may be sealed with it. Defaults to "the installed AE runtime folder, no
/// ceiling" (issue #304).
#[derive(Clone, Default)]
struct DependencyConfig {
    dirs: Vec<PathBuf>,
    module_limit: Option<usize>,
    byte_limit: Option<u64>,
}

/// The default dependency folders: the newest installed After Effects
/// `Support Files\`, which is where an AE effect's Adobe runtime DLLs
/// (`dvacore.dll` and friends) live, one level above the `Plug-ins\` tree that
/// is scanned for effects.
fn default_dependency_dirs() -> Vec<PathBuf> {
    latest_after_effects_plugins()
        .0
        .and_then(|plugins| plugins.parent().map(Path::to_path_buf))
        .filter(|support_files| support_files.is_dir())
        .into_iter()
        .collect()
}

/// The search roots for one AEX: its own folder first (an AEX that ships its
/// helper DLLs beside itself resolves them the way the installed host would),
/// then the configured runtime folders.
///
/// Each root is canonicalized here, because the resolver requires absolute roots
/// — a root whose meaning depends on the process working directory is exactly
/// what it should refuse — while the config may legitimately be written relative.
/// A folder that cannot be canonicalized (missing, or not a directory) is dropped
/// rather than failing the resolution: it can never provide a DLL, so keeping it
/// would only turn a stale config line into "nothing discovers at all". Too
/// *many* folders is not softened — the resolver rejects that, so a config over
/// the root limit fails loudly instead of silently ignoring the tail.
fn search_roots_for(plugin: &Path, dependency_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let canonical_dir = |dir: &Path| {
        std::fs::canonicalize(dir)
            .ok()
            .filter(|canonical| canonical.is_dir())
    };
    let mut roots: Vec<PathBuf> = plugin
        .parent()
        .and_then(canonical_dir)
        .into_iter()
        .collect();
    for dir in dependency_dirs {
        if let Some(dir) = canonical_dir(dir)
            && !roots.iter().any(|root| root == &dir)
        {
            roots.push(dir);
        }
    }
    roots
}

/// The dependency closure sealed with `plugin`, or an error string.
///
/// Failing here is not softened into "no dependencies": a plug-in whose closure
/// cannot be resolved would only fail again inside the worker as an opaque
/// `LoadLibraryExW` failure, so the reason is kept and surfaced by the caller.
fn dependency_closure_for(
    plugin: &Path,
    dependency: &DependencyConfig,
    roots: &[PathBuf],
) -> Result<ResolvedDependencyClosure, String> {
    resolve_dependency_closure(DependencyClosureRequest {
        max_dependencies: dependency.module_limit,
        max_total_bytes: dependency.byte_limit,
        ..DependencyClosureRequest::new(plugin, roots)
    })
    .map_err(|error| format!("dependency closure resolution failed: {error}"))
}

/// Whether a name is a Windows API set, which the loader resolves on its own.
fn is_api_set(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("api-ms-") || name.starts_with("ext-ms-")
}

/// `(path, mtime, len)` for each dependency file, plus the basenames of any that
/// vanished between the walk and here.
///
/// A file that is already gone cannot be compared against later, but its
/// disappearance is exactly the kind of change that should re-verify the entry —
/// so it is handed back as a name nothing provides. If it stays gone the entry
/// converges (no root offers it); if it comes back, the missing-name check fires.
fn cached_dependencies(paths: &[PathBuf]) -> (Vec<CachedDependency>, Vec<String>) {
    let mut dependencies = Vec::with_capacity(paths.len());
    let mut vanished = Vec::new();
    for path in paths {
        match file_meta(path) {
            Some((mtime, len)) => dependencies.push(CachedDependency {
                path: path.to_string_lossy().into_owned(),
                mtime,
                len,
            }),
            None => vanished.extend(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_lowercase),
            ),
        }
    }
    (dependencies, vanished)
}

/// The imported names worth re-checking at startup: everything no search root
/// provided, minus the Windows API sets the loader owns.
fn cached_missing(unresolved: &[String]) -> Vec<String> {
    unresolved
        .iter()
        .filter(|name| !is_api_set(name))
        .cloned()
        .collect()
}

/// `%ProgramFiles%\Adobe`, the root of Adobe app installs.
fn adobe_root() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles")?;
    Some(PathBuf::from(program_files).join("Adobe"))
}

/// The newest `Adobe After Effects <year>\Support Files\Plug-ins`, or `None`.
fn latest_after_effects_plugins() -> (Option<PathBuf>, bool) {
    let Some(adobe) = adobe_root() else {
        return (None, false);
    };
    newest_versioned(
        &adobe,
        "Adobe After Effects ",
        &["Support Files", "Plug-ins"],
    )
}

/// The newest `Adobe\Common\Plug-ins\<version>\MediaCore`, or `None`.
fn mediacore_dir() -> (Option<PathBuf>, bool) {
    let Some(adobe) = adobe_root() else {
        return (None, false);
    };
    let root = adobe.join("Common").join("Plug-ins");
    newest_versioned(&root, "", &["MediaCore"])
}

/// The `leaf` folder under the newest versioned subfolder of `root` whose name
/// starts with `prefix` (e.g. `Adobe After Effects 2025/Support Files/Plug-ins`).
///
/// The second value is false when the pick cannot be trusted to be the newest:
/// the folder could not be enumerated, an entry could not be read, or a version
/// *newer than the pick* was present without its `leaf`. That last case is what
/// an install being updated looks like, and silently falling back to an older
/// version while reporting a complete scan would make the newer version's
/// plug-ins look deleted — which prunes their cache entries and unregisters them
/// on the next launch, deleting objects from saved projects (issue #307). A
/// leafless *older* version is just an uninstall leftover and means nothing.
fn newest_versioned(root: &Path, prefix: &str, leaf: &[&str]) -> (Option<PathBuf>, bool) {
    let Ok(read) = std::fs::read_dir(root) else {
        return (None, false);
    };
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    let mut leafless: Vec<Vec<u64>> = Vec::new();
    let mut complete = true;
    for entry in read {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(version) = name.strip_prefix(prefix) else {
            continue;
        };
        // A numbered name is what an install in progress looks like. Unnumbered
        // ones (`... (Beta)`) are still picked when nothing numbered exists, but a
        // missing leaf under them is not evidence of an incomplete install.
        let numbered = version
            .split(['.', ' '])
            .any(|part| part.parse::<u64>().is_ok());
        let key = version_key(version);
        let mut candidate = entry.path();
        // Tested through the path, not `DirEntry::file_type`, which reports a
        // directory junction as a symlink rather than a directory — Adobe installs
        // are routinely junctioned to another drive.
        if !candidate.is_dir() {
            // A plain file is clutter. A reparse point that will not resolve is an
            // install we simply could not see this launch, which must not read as
            // "its plug-ins are gone".
            let unresolved = candidate
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink());
            if numbered && unresolved {
                leafless.push(key);
            }
            continue;
        }
        candidate.extend(leaf);
        if !candidate.is_dir() {
            if numbered {
                leafless.push(key);
            }
            continue;
        }
        if best.as_ref().is_none_or(|(best_key, _)| key > *best_key) {
            best = Some((key, candidate));
        }
    }
    // A leafless version above the pick means the newest install is not fully
    // visible this launch, so its absence is not evidence its plug-ins are gone.
    let best_key = best.as_ref().map(|(key, _)| key);
    complete &= !leafless
        .iter()
        .any(|key| best_key.is_none_or(|best_key| key > best_key));
    (best.map(|(_, path)| path), complete)
}

/// Why a launch's picture of what is on disk is not authoritative. Each cause
/// suppresses the prune the same way, but they need different fixes from the
/// user, so they are reported apart rather than as one "incomplete" (issue #660:
/// the depth cap was being reported as a folder that could not be read).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct ScanLimits {
    /// A *default* scan folder could not be resolved (an AE install mid-update).
    unresolved_root: bool,
    /// A folder or one of its entries could not be read this launch.
    unreadable: bool,
    /// A subtree went past [`MAX_SCAN_DEPTH`] and was not descended.
    too_deep: bool,
}

impl ScanLimits {
    /// Whether this launch may conclude that an AEX it did not see is gone.
    fn authoritative(self) -> bool {
        !self.unresolved_root && !self.unreadable && !self.too_deep
    }

    fn merge(&mut self, other: Self) {
        self.unresolved_root |= other.unresolved_root;
        self.unreadable |= other.unreadable;
        self.too_deep |= other.too_deep;
    }

    /// What to tell the user, or `None` when the scan was authoritative. Every
    /// applicable cause is listed: they are independent and fixed differently.
    fn describe(self) -> Option<String> {
        self.causes(false)
    }

    /// The same, with the remedy attached to the cause it belongs to. Written per
    /// cause rather than appended once, because a trailing "check the path"
    /// after a list binds to whichever cause happens to be last — and the depth
    /// cap is not a path the user can check (issue #660).
    fn describe_with_remedy(self) -> Option<String> {
        self.causes(true)
    }

    fn causes(self, remedy: bool) -> Option<String> {
        let path_remedy = if remedy {
            format!(
                " (check the path exists and is reachable — `dir`/`dirs` in \
                 config.toml, or {ENV_DIR} if that is set)"
            )
        } else {
            String::new()
        };
        let mut causes: Vec<String> = Vec::new();
        if self.unresolved_root {
            causes.push(format!(
                "a default plug-in folder could not be resolved{path_remedy}"
            ));
        }
        if self.unreadable {
            causes.push(format!("a folder could not be read{path_remedy}"));
        }
        if self.too_deep {
            // No remedy: the cap is not reachable from config, and offering one
            // is the misdirection this issue is about.
            causes.push("a folder tree was deeper than the scan limit".to_owned());
        }
        (!causes.is_empty()).then(|| causes.join("; "))
    }
}

/// What one launch's folder scan saw.
struct Scan {
    /// The AEX to expose as filters (ignored ones removed).
    plugins: Vec<PathBuf>,
    /// Every `*.aex` seen, ignored ones included. This, not `plugins`, is what
    /// the prune may judge existence from: an ignored AEX is present on disk, and
    /// dropping its cache entry would leave it unregistered on the launch after
    /// it is taken back out of `ignore` (issue #307).
    seen: Vec<PathBuf>,
    /// Why the walk itself could not be exhaustive, if it could not. Never
    /// carries `unresolved_root`: that is about which folders were handed to the
    /// scan, which only the caller knows. Fold it in before judging authority.
    limits: ScanLimits,
}

/// Recursively scans `dirs`. An incomplete scan (a folder that could not be read,
/// a tree deeper than [`MAX_SCAN_DEPTH`]) must not be used to conclude an AEX is
/// gone: pruning its cache entry would leave the effect unregistered on the next
/// launch, which deletes objects from saved projects that use it (issue #307).
fn collect_aex(dirs: &[PathBuf], ignore: &[String]) -> Scan {
    let mut seen = Vec::new();
    let mut limits = ScanLimits::default();
    // Shared across roots: junctions can make one folder reachable from several
    // of them, and descending twice would expose the same AEX as several filters.
    let mut visited = std::collections::HashSet::new();
    for dir in dirs {
        limits.merge(collect_aex_into(dir, 0, &mut seen, &mut visited));
    }
    seen.sort();
    seen.dedup();
    let plugins = seen
        .iter()
        .filter(|path| !is_ignored(path, ignore))
        .cloned()
        .collect();
    Scan {
        plugins,
        seen,
        limits,
    }
}

/// Reports which parts of this subtree could not be enumerated, if any.
fn collect_aex_into(
    dir: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
    visited: &mut std::collections::HashSet<PathBuf>,
) -> ScanLimits {
    if depth > MAX_SCAN_DEPTH {
        return ScanLimits {
            too_deep: true,
            ..ScanLimits::default()
        };
    }
    // A junction can point back up the tree or make one folder reachable twice.
    // Visiting the real folder once keeps an AEX from being registered as several
    // filters (each with its own discovery worker). Already visited means "seen",
    // not "not looked at", so it does not make the scan incomplete.
    if let Ok(real) = dir.canonicalize()
        && !visited.insert(real)
    {
        return ScanLimits::default();
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        // The path every mistyped `dir`, unmounted drive and denied folder takes:
        // `collect_aex` learns of a missing folder only as a failed `read_dir`.
        return ScanLimits {
            unreadable: true,
            ..ScanLimits::default()
        };
    };
    let mut limits = ScanLimits::default();
    for entry in read {
        // An entry the iterator itself could not yield is a partially enumerated
        // folder; flattening it away would report the scan as complete and let
        // the prune drop that AEX's entry (issue #307).
        let Ok(entry) = entry else {
            limits.unreadable = true;
            continue;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            limits.unreadable = true;
            continue;
        };
        // A directory junction reports as a symlink, not a directory, so testing
        // only `is_dir()` would silently skip a junctioned subfolder while still
        // calling the scan complete — and the prune would then delete the cache
        // entries of every AEX under it, unregistering them (issue #307).
        // A link cycle is broken by `visited` above, not by the depth cap.
        let resolved = file_type.is_symlink().then(|| std::fs::metadata(&path));
        if matches!(resolved, Some(Err(_))) {
            // A link whose target cannot be resolved (its drive is not mounted
            // this launch) says nothing about what is behind it. Treating that as
            // "no AEX here" would prune everything under it.
            limits.unreadable = true;
            continue;
        }
        if file_type.is_dir() || matches!(&resolved, Some(Ok(meta)) if meta.is_dir()) {
            limits.merge(collect_aex_into(&path, depth + 1, out, visited));
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("aex"))
        {
            out.push(path);
        }
    }
    limits
}

// --- Per-AEX discovery + registration ------------------------------------

/// Reads one config item's current (keyframed) value back into the parameter it
/// drives. AviUtl2 updates each item struct's value right before proc_video.
enum ItemReader {
    Track {
        ptr: *const FILTER_ITEM_TRACK,
        slot: u32,
        integer: bool,
    },
    Checkbox {
        ptr: *const FILTER_ITEM_CHECKBOX,
        slot: u32,
    },
    Select {
        ptr: *const FILTER_ITEM_SELECT,
        slot: u32,
    },
    Color {
        ptr: *const FILTER_ITEM_COLOR,
        slot: u32,
    },
}

/// The launch-fixed geometry/time of a session (the AEX identity is fixed per
/// FilterCtx). A frame whose object geometry or timing differs needs a fresh
/// session, so this is compared per frame.
#[derive(Clone, PartialEq, Eq, Hash)]
struct GeomIdentity {
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
}

/// A single validated frame handed back to the AviUtl2 callback thread.
struct RenderedFrame {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// The outcome of one frame, distinguishing a still-usable session from a lost
/// one so the caller reopens only when necessary.
enum FrameReply {
    Rendered(RenderedFrame),
    /// A frame-local diagnostic; the session stays usable, leave pixels.
    FrameLocal(i64),
    /// The session/worker is gone; the caller drops it so the next frame reopens.
    SessionLost(String),
}

/// A render request sent to a session's owning thread.
struct RenderReq {
    current_time: i32,
    rgba: Vec<u8>,
    parameters: Option<Vec<InteractiveParameter>>,
    /// This frame's virtual-buffer pixels for the AEX's layer slot, when the
    /// session opened one as dynamic (issue #674). `None` leaves the layer
    /// showing whatever it last held, which is what a session without a layer
    /// input, or a frame whose buffer could not be read, wants.
    layer: Option<(u32, Vec<u8>)>,
    /// The manifest index this frame's plugin holds inside a pooled cluster
    /// session (issue #405); always 0 for a single-plugin session, which
    /// never swaps.
    plugin_index: u32,
    reply: Sender<FrameReply>,
}

/// Handle to a resident session. `RenderSession` is `!Send` (it holds the
/// shared-memory view pointer), so it stays pinned to `join`'s thread and is
/// reached only through `tx`. This handle is `Send`, so the per-AEX session map
/// can live behind a `Mutex` reached from any AviUtl2 callback thread.
struct MfSession {
    tx: Option<Sender<RenderReq>>,
    identity: GeomIdentity,
    serial: u64,
    last_used: Instant,
    join: Option<JoinHandle<()>>,
}

impl MfSession {
    fn sender(&self) -> Option<Sender<RenderReq>> {
        self.tx.clone()
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        // Drop the sender first so the owning thread's `rx.recv()` returns and it
        // closes the session; joining before disconnecting would deadlock.
        self.tx = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// A render session whose close-time validation failed (issue #405, design
/// §6/§7). Every session thread inspects its `RenderSession::close` summary;
/// when the close-time module audit, teardown, or worker-exit checks fail,
/// the failure is recorded here instead of being dropped — a frame the
/// session delivered must never stand as an untracked success ("never round
/// a failure into a success"). Bounded to the most recent failures.
#[derive(Clone, Debug)]
struct SessionCloseFailure {
    /// Identity of the plug-in the session last had loaded, exactly as
    /// approved at open.
    plugin: PathBuf,
    plugin_sha256: String,
    smart: bool,
    clustered: bool,
    /// Why the close was not clean: the session invalidation reason (e.g.
    /// `module_audit_mismatch`), or the worker exit classification.
    close_reason: String,
    /// The close-time module audit outcome when the worker left a final
    /// report (the audit status or the invalidation detail).
    module_audit: Option<String>,
    /// Identity of the worker image that ran the session, hashed at close.
    worker: PathBuf,
    worker_sha256: Option<String>,
    frames_ok: u32,
    frames_errored: u32,
    /// What the bridge does about it: the failed session is gone, so the
    /// next frame for this plug-in opens a fresh worker process (design §6).
    fallback: &'static str,
}

/// The recent close-time session failures, oldest first (bounded).
static SESSION_CLOSE_FAILURES: Mutex<Vec<SessionCloseFailure>> = Mutex::new(Vec::new());

/// Inspects the close summary every session thread produces and records a
/// non-clean close as a structured failure (issue #405 review): the session
/// is treated as invalidated, and its frames are not left as a silent
/// success. An in-flight frame at failure time already resolves to
/// `SessionLost` in the request loop; this record is what keeps a close-time
/// failure discovered afterwards — a teardown, audit, or worker-exit failure
/// with no request in flight — from being dropped.
fn record_session_close(config: &MfSessionConfig, close: &serde_json::Value) {
    if close.get("session_clean") == Some(&serde_json::Value::Bool(true)) {
        return;
    }
    let as_u32 = |key: &str| {
        close
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32
    };
    let close_reason = close
        .get("invalidated_reason")
        .and_then(|reason| reason.get("reason"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            close
                .get("worker")
                .and_then(|worker| worker.get("classification"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("session_clean=false")
                .to_owned()
        });
    let module_audit = close
        .get("invalidated_reason")
        .and_then(|reason| reason.get("detail"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            close
                .get("final_report")
                .and_then(|report| report.get("module_audit"))
                .and_then(|audit| audit.get("status"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    let worker = config.repository.join(if config.smart {
        "target/minihost-build/aex_smart_worker.exe"
    } else {
        "target/minihost-build/aex_render_worker.exe"
    });
    let worker_sha256 = std::fs::read(&worker)
        .ok()
        .map(|bytes| hex_lower(&Sha256::digest(&bytes)));
    let failure = SessionCloseFailure {
        plugin: config.plugin.clone(),
        plugin_sha256: config.sha.clone(),
        smart: config.smart,
        clustered: config.cluster.is_some(),
        close_reason,
        module_audit,
        worker,
        worker_sha256,
        frames_ok: as_u32("frames_ok"),
        frames_errored: as_u32("frames_errored"),
        fallback: "reopen_fresh_session",
    };
    if let Ok(mut failures) = SESSION_CLOSE_FAILURES.lock() {
        if failures.len() >= 32 {
            failures.remove(0);
        }
        failures.push(failure);
    }
}

/// Per-filter userdata carried by the libffi closure. One per registered AEX,
/// captured by that AEX's single closure and reached as `&FilterCtx` through the
/// C callback boundary from any AviUtl2 callback thread.
///
/// `FilterCtx` is not auto-`Send + Sync` (the `ItemReader`s hold `*const
/// FILTER_ITEM_*` raw pointers, which are `!Sync`), and no `unsafe impl` claims
/// otherwise — the C boundary erases the check. Sharing it across threads is
/// nonetheless sound because: the only genuinely thread-unsafe state, the
/// `!Send` `RenderSession`, never leaves its owning session thread (only the
/// `Send` `Sender<RenderReq>` crosses threads); the session map is behind a
/// `Mutex`; and the raw item pointers address leaked `'static` (process-global)
/// memory that is only ever read in `apply_readers`, on the calling AviUtl2
/// thread. Do not move `readers`/`apply_readers` onto the session thread.
struct FilterCtx {
    repository: PathBuf,
    plugin: PathBuf,
    /// Where this AEX's dependency DLLs are looked for when its session opens,
    /// and any ceilings on them; the same configuration discovery used (#304).
    dependency: DependencyConfig,
    sha: String,
    smart: bool,
    /// This AEX's dependency-closure identity from discovery (issue #405).
    /// When other registered AEXes share it, renders route through the pooled
    /// cluster session instead of a per-effect worker.
    closure_identity: Option<String>,
    /// Exposed parameter defaults (normalized), cloned per frame as the baseline.
    defaults: Vec<InteractiveParameter>,
    /// Readers pulling each frame's current config value into the parameters.
    readers: Vec<ItemReader>,
    /// AEX layer-parameter slots (`kind == "layer"`) from discovery. When this is
    /// non-empty and AviUtl2's virtual buffer is written upstream, the first slot
    /// is fed the virtual buffer as a `SessionLayer` at session open (issue #645).
    layer_slots: Vec<u32>,
    /// Live sessions keyed by AviUtl2 `effect_id`, so two objects of the same
    /// AEX filter each get their own session/worker (no cross-object thrash).
    sessions: Mutex<HashMap<i64, MfSession>>,
}

/// The cached discovery result for one AEX. Keyed in the cache file by the AEX
/// path; `(mtime, len)` invalidates the entry when the file changes. `ok` records
/// a non-discoverable `.aex` (e.g. a format/codec plug-in, not an effect) so it
/// is skipped without being re-probed every launch.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    mtime: (u64, u32),
    len: u64,
    ok: bool,
    sha: String,
    smart: bool,
    #[serde(default)]
    params: Vec<InteractiveParameter>,
    /// The host build that produced this entry. Held per entry, not per file, so
    /// an interrupted re-verification pass leaves the not-yet-redone entries
    /// carrying the old build and they are queued again on the next launch.
    #[serde(default)]
    build: BuildFingerprint,
    /// Set when the AEX changed while it was being discovered, so `sha`/`params`
    /// may describe the previous bytes. The entry is still registered (better
    /// than unregistering it — issue #307) but is always re-discovered, so it
    /// cannot stay permanently wrong.
    #[serde(default)]
    stale: bool,
    /// The host that last *attempted* to re-verify this entry, which is not the
    /// one that produced it when the attempt failed. Kept apart from `build` so a
    /// failed attempt cannot pass the payload off as the current host's work:
    /// doing that both hides its real provenance and silently ends re-verification
    /// for that host, leaving an effect on an older host's parameters.
    #[serde(default)]
    checked: BuildFingerprint,
    /// Failed re-verification attempts under `checked`, so retries are bounded
    /// (see [`RETRY_BUDGET`]) instead of running on every launch forever.
    #[serde(default)]
    attempts: u8,
    /// What this entry's dependency resolution saw (issue #304), so the entry can
    /// be re-verified when that changes. Added additively: an entry written
    /// before this field simply has no roots recorded, which reads as "resolved
    /// differently" and queues it for the background pass — it is never dropped,
    /// and it stays registered meanwhile (issue #307).
    #[serde(default)]
    closure: CachedClosure,
    /// Classification of the most recent failed discovery attempt. Timeout
    /// classes are deliberately retained so transient runner pressure cannot
    /// demote a valid stale entry; deterministic worker failures may converge
    /// it to a negative cache entry (#328).
    #[serde(default)]
    failure_classification: Option<String>,
    /// This spelling is retained only as a fallback after an alias re-key. It
    /// must not keep the alias lookup hot while its walked spelling is present.
    #[serde(default)]
    alias_fallback: bool,
    /// The walked spelling copied from this fallback, if it is still cached.
    #[serde(default)]
    alias_target: Option<String>,
    /// Normalized hash of the resolved dependency closure (issue #405):
    /// effects sharing it can be discovered/rendered through one cluster
    /// session. `None` when the closure never resolved (such an entry cannot
    /// cluster and always takes the per-plugin path).
    #[serde(default)]
    closure_identity: Option<String>,
    /// Structured record of a cluster-session fallback (issue #405, design
    /// §6): present when this entry was produced after a cluster discovery
    /// session failed — never silently rounded into a plain success.
    #[serde(default)]
    cluster_fallback: Option<ClusterFallback>,
}

/// How one cache entry relates to a failed cluster discovery session
/// (issue #405, design §6).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ClusterFallback {
    /// 0-based member index within the cluster session at which it was
    /// invalidated (0 when the session never opened).
    at_member: u32,
    /// The session failure reason (broker invalidation reason, the
    /// inspect/swap error kind, or "cluster_infeasible").
    reason: String,
    /// How this entry was produced instead: `invalidated` for the member the
    /// session died on (recorded as a failure), `one_shot_fallback` for a
    /// member re-inspected through the per-plugin path.
    resolution: String,
}

/// The resolution behind one cache entry: where it looked, what it sealed, and
/// what it could not find.
///
/// A discovery result depends on all three. Re-checking them costs a handful of
/// `stat` calls per entry, which is what lets the plug-in scan stay cheap at
/// startup while still re-verifying an entry whose closure would now differ.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct CachedClosure {
    /// Search roots in resolution order (first match wins, like the loader).
    #[serde(default)]
    roots: Vec<String>,
    /// The dependency files the resolution reached, as `(path, mtime, len)`:
    /// what was sealed when it succeeded, what it would have sealed when it
    /// failed. Recording them either way is what lets a failure caused by the
    /// dependencies themselves — an operator's ceiling exceeded, say — be redone
    /// once those files change.
    #[serde(default)]
    sealed: Vec<CachedDependency>,
    /// Imported names no search root provided, whether the loader then found them
    /// in System32 or not at all. Both matter the same way: if a root starts
    /// providing one, the closure changes.
    ///
    /// Windows API sets (`api-ms-*`, `ext-ms-*`) are left out. The loader owns
    /// those names and a plug-in folder cannot take them over, so tracking them
    /// would only cost startup `stat` calls.
    #[serde(default)]
    missing: Vec<String>,
    /// Resolver provenance for every sealed basename. This is diagnostic-only:
    /// worker module audit still classifies each authenticated file through the
    /// existing plug-in-tree policy. Additive/defaulted so pre-#360 caches remain
    /// readable and are naturally refreshed by the rebuilt host fingerprint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    provenance: Vec<CachedDependencyProvenance>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CachedDependencyProvenance {
    basename: String,
    import_derived: bool,
    string_derived: bool,
}

/// One sealed dependency's identity, as cheap to re-check as a `stat`.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CachedDependency {
    path: String,
    mtime: (u64, u32),
    len: u64,
}

fn cached_provenance(sources: &[DependencyProvenance]) -> Vec<CachedDependencyProvenance> {
    sources
        .iter()
        .map(|source| CachedDependencyProvenance {
            basename: source.basename.clone(),
            import_derived: source.import_derived,
            string_derived: source.string_derived,
        })
        .collect()
}

/// Whether this entry should be re-discovered because its dependency closure
/// would now resolve differently (issue #304).
///
/// Bounded by the same [`RETRY_BUDGET`] a host change is: a re-discovery that
/// keeps failing does not update the recorded closure (`keep_best` refuses to
/// demote a working entry and keeps its record with it), so without the budget an
/// AE update that rewrites one runtime DLL would re-run a worker for every effect
/// on every launch, forever.
fn needs_closure_recheck(entry: &CacheEntry, build: BuildFingerprint, roots: &[PathBuf]) -> bool {
    (entry.checked != build || entry.attempts < RETRY_BUDGET)
        && !closure_still_resolves_the_same(entry, roots)
}

/// Whether re-resolving this entry's closure today would still reach the same
/// files, judged with `stat` only. `roots` is what the resolution would search
/// now, in order.
///
/// Four things can change the answer without touching the AEX itself, and each is
/// checked here:
///
/// 1. the search roots themselves differ from the ones the entry was resolved
///    against — a configured folder that moved, or a relative one now resolving
///    elsewhere because the host started from a different working directory,
/// 2. a sealed dependency was rewritten or removed (an AE update rewriting
///    `dvacore.dll` in place, a helper `*.aex` replaced),
/// 3. a file appeared in an earlier search root and now wins a name that used to
///    resolve further down the order,
/// 4. a search root now provides a name that no root provided at discovery time —
///    which turns a cached failure into a plug-in that would load, and equally
///    turns a System32 fallback into an app-local DLL that would be sealed.
///
/// A false "changed" only re-verifies the plug-in in the background; the entry
/// stays registered either way (issue #307).
fn closure_still_resolves_the_same(entry: &CacheEntry, roots: &[PathBuf]) -> bool {
    if entry.closure.roots.len() != roots.len()
        || !entry
            .closure
            .roots
            .iter()
            .zip(roots)
            .all(|(recorded, current)| Path::new(recorded) == current.as_path())
    {
        return false;
    }
    for dependency in &entry.closure.sealed {
        let path = Path::new(&dependency.path);
        if !file_meta(path)
            .is_some_and(|(mtime, len)| mtime == dependency.mtime && len == dependency.len)
        {
            return false;
        }
        let Some(name) = path.file_name() else {
            return false;
        };
        for root in roots {
            if path.parent() == Some(root.as_path()) {
                break;
            }
            if root.join(name).is_file() {
                return false;
            }
        }
    }
    !entry
        .closure
        .missing
        .iter()
        .any(|name| roots.iter().any(|root| root.join(name).is_file()))
}

/// How many times a re-verification may fail for one host build before the entry
/// is left alone until the host changes again.
///
/// Above one so a single transient failure — a worker timeout under load — does
/// not strand an entry on an older host's parameters. Small, because a host that
/// genuinely cannot discover an effect any more would otherwise re-run a worker
/// for it on every launch, and a regression can put hundreds of entries in that
/// state at once.
const RETRY_BUDGET: u8 = 3;

/// Invalidates cache files whose [`CacheEntry`] shape can no longer be trusted
/// field-for-field.
///
/// **Avoid bumping this.** A bump discards every entry, so that launch registers
/// no filters at all, and opening a saved project that uses them makes AviUtl2
/// drop those objects — saving then deletes them for good (issue #307). Extend
/// the schema additively instead: a new field with `#[serde(default)]` reads old
/// cache files safely and needs no bump (this is how `CacheEntry::build` was
/// added). Bump only if an existing field's meaning changes, which is a real
/// data-loss risk that has to be weighed rather than done reflexively.
const CACHE_VERSION: u32 = 1;
/// A save is short, but another AviUtl2 process may be between its read and
/// atomic replace.  Serialize the read/merge/write critical section with a
/// Windows handle lock so every writer observes the previous writer's result.
const CACHE_LOCK_RETRIES: usize = 200;
const CACHE_LOCK_RETRY: Duration = Duration::from_millis(10);

/// Fingerprints the compat host that produces a discovery result, so an entry can
/// be re-verified when the host changes (e.g. it gains support for an effect that
/// previously failed to load — issue #304). A cached result depends on the host,
/// not just the AEX bytes: on the L2 worker exe that loads the AEX and runs
/// `EffectMain`, and on this multifilter DLL, whose in-process broker does the
/// sealed-load-tree staging and dispatch that decide whether a load even succeeds.
/// `(mtime_secs, mtime_nanos, len)` per file; `None` when a file cannot be stat'd.
#[derive(serde::Serialize, serde::Deserialize, Default, PartialEq, Eq, Clone, Copy, Debug)]
struct BuildFingerprint {
    #[serde(default)]
    worker: Option<(u64, u32, u64)>,
    #[serde(default)]
    host: Option<(u64, u32, u64)>,
    /// Digest of the closure ceilings (issue #304). They decide whether a closure
    /// is sealed at all, so an entry produced under different ones is re-verified
    /// like one produced by an older host. The search folders and what they
    /// contain are tracked per entry instead, in [`CachedClosure`].
    #[serde(default)]
    dependency_inputs: u64,
}

impl BuildFingerprint {
    /// Whether both halves were actually stat'd. An unknown fingerprint must not
    /// count as "a different host", or one transient stat failure re-discovers
    /// every AEX twice: once under the unknown build, once when it resolves again.
    fn is_known(&self) -> bool {
        self.worker.is_some() && self.host.is_some()
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CacheFile {
    version: u32,
    /// Held as raw JSON, and converted per entry on load, so one entry that no
    /// longer deserializes drops only itself instead of emptying the cache and
    /// unregistering every filter for a launch (issue #307).
    ///
    /// This contains *isolated* damage only. [`CacheEntry::params`] embeds
    /// `InteractiveParameter` from the broker crate, whose fields are not all
    /// `#[serde(default)]`, so a field added there fails every entry that has
    /// parameters — i.e. every registerable filter — at once. That shared
    /// dependency is pinned by `the_cached_parameter_schema_is_stable`, which
    /// fails in `cargo test` rather than letting the change reach users' caches.
    entries: HashMap<String, serde_json::Value>,
}

/// The discovery cache path, next to the config in `%APPDATA%`.
fn cache_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("aexcompat-multifilter")
            .join("discovery-cache.json"),
    )
}

/// Fingerprints the L2 discovery worker and this multifilter DLL. Either side can
/// change a discovery result: the worker exe loads the AEX and runs `EffectMain`,
/// while the in-DLL broker does the sealed-load-tree staging that decides whether
/// the load succeeds (issue #304's dep-sealing lives broker-side, in this DLL).
fn build_fingerprint(repository: &Path, dependency: &DependencyConfig) -> BuildFingerprint {
    let worker = repository.join(L2_WORKER_RELATIVE_PATH);
    let flatten = |m: ((u64, u32), u64)| (m.0.0, m.0.1, m.1);
    BuildFingerprint {
        worker: file_meta(&worker).map(flatten),
        host: self_module_path()
            .as_deref()
            .and_then(file_meta)
            .map(flatten),
        dependency_inputs: dependency_inputs_fingerprint(dependency),
    }
}

/// A digest of the ceilings, which decide whether a closure is sealed at all and
/// are not recorded per entry. Only equality matters, so the leading 8 bytes of
/// the SHA-256 are enough and keep the fingerprint `Copy`.
///
/// The search folders are deliberately **not** hashed here, even though they
/// decide the outcome too. Each entry records the canonical roots it actually
/// resolved against and is compared against today's, which is both exact (a
/// relative config string can mean different folders on different launches) and
/// per-entry. Hashing the folders instead would make one global value out of a
/// resolution that is not global — and `default_dependency_dirs` can legitimately
/// return the previous AE version's folder while an update is in flight, which
/// would then queue every entry for re-verification against the wrong runtime.
fn dependency_inputs_fingerprint(dependency: &DependencyConfig) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"limits\0");
    hasher.update(dependency.module_limit.unwrap_or(usize::MAX).to_le_bytes());
    hasher.update(dependency.byte_limit.unwrap_or(u64::MAX).to_le_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap_or_default())
}

/// Where the compat-host workers live, as the root the broker joins
/// `target/minihost-build/aex_*_worker.exe` onto.
///
/// Preference order:
///
/// 1. `AEXCOMPAT_MULTIFILTER_REPOSITORY`, then the TOML `repository` — a
///    developer naming their checkout. Taken even when no worker is built there
///    yet, since that is the tree they are about to build in.
/// 2. Beside the plugin: this DLL's own folder, then `<dll folder>/aexcompat`,
///    each accepted only when the L2 worker is actually present.
///
/// (2) is what lets a deployed plugin carry its own workers. Pointing the plugin
/// at a source tree makes its runtime depend on a directory the user is free to
/// delete, move, or `cargo clean` — which is how a removed worktree turned into
/// "every AEX fails discovery, zero filters registered" with nothing reported
/// (issue #650).
///
/// A named checkout that no longer holds a worker falls through to (2) rather
/// than being handed to the broker regardless. Preferring the setting is right
/// while it points at something usable; keeping that preference after the tree
/// is gone only reproduces the incident. The returned [`WorkerRootSource`] is
/// what lets the caller say which of the two happened (issue #655).
fn resolve_worker_root(
    env_override: Option<PathBuf>,
    configured: Option<&Path>,
    module_path: Option<PathBuf>,
) -> Option<(PathBuf, WorkerRootSource)> {
    let named: Vec<PathBuf> = env_override
        .into_iter()
        .chain(configured.map(Path::to_path_buf))
        .collect();
    let beside: Vec<PathBuf> = module_path
        .as_deref()
        .and_then(Path::parent)
        .map(|dir| vec![dir.to_path_buf(), dir.join("aexcompat")])
        .unwrap_or_default();

    if let Some(root) = named
        .iter()
        .chain(beside.iter())
        .find(|root| root.join(L2_WORKER_RELATIVE_PATH).is_file())
    {
        let source = if named.iter().any(|named| named == root) {
            WorkerRootSource::Named
        } else {
            WorkerRootSource::BesidePlugin
        };
        return Some((root.clone(), source));
    }
    // Nothing holds a worker: keep a named checkout (it may be about to be
    // built), but never invent one from the plugin's folder.
    named
        .into_iter()
        .next()
        .map(|root| (root, WorkerRootSource::NamedWithoutWorker))
}

/// The line for a launch that found no worker anywhere.
///
/// Built from the same constants the search uses, so the remediation cannot drift
/// from what is actually probed. The workers sit under a `target/minihost-build/`
/// subtree, so "put the exe beside the plugin" would send the user to a folder
/// the search never looks in.
fn missing_worker_advice() -> String {
    format!(
        "no compat host worker found: set `repository` in config.toml (or \
         {ENV_REPOSITORY}), or place the workers at <plugin folder>\\{} — an \
         `aexcompat` subfolder there works too. No AEX filter is registered.",
        // Windows separators throughout: the constant is `/`-separated for
        // `Path::join`, and a half-converted path reads like a typo.
        L2_WORKER_RELATIVE_PATH.replace('/', "\\")
    )
}

/// Says which worker root this launch resolved and how, and which folders it
/// will scan. Emitted before the scan so an early return still leaves the log
/// showing what the plugin decided.
fn report_worker_root(repository: &Path, source: WorkerRootSource, dirs: &[PathBuf]) {
    let line = format!(
        "worker root: {} ({}); scanning {}",
        repository.display(),
        source.describe(),
        describe_dirs(dirs)
    );
    // A named root with no worker in it cannot register anything, and that is the
    // shape of the incident behind issue #650 (a `repository` left pointing at a
    // deleted worktree). Say so at load rather than leaving it to be inferred.
    if source == WorkerRootSource::NamedWithoutWorker {
        log_warn(&format!(
            "{line}. No worker is built there, so discovery will fail for every \
             plug-in until one is."
        ));
    } else {
        log_info(&line);
    }
}

/// The line for a launch that registered nothing because the scan produced no
/// plug-in.
///
/// It names the folders rather than guessing why they held nothing. A mistyped
/// folder and one that exists but cannot be read are indistinguishable here —
/// `collect_aex` learns of a missing folder only as a failed `read_dir` — so
/// guessing would have sent a user with a typo away to wait for a transient
/// problem to clear.
fn empty_scan_summary(dirs: &[PathBuf], seen: usize, limits: ScanLimits) -> String {
    // Everything the scan walked matched `ignore`. That is the whole cause of
    // what it *did* see, but a folder it could not walk hid its contents from the
    // ignore list too, so the tail still admits that.
    if seen > 0 {
        return format!(
            "all {seen} .aex found are excluded by `ignore` in config.toml; no AEX \
             filter is registered{}",
            scan_limit_note(limits)
        );
    }
    format!(
        "no .aex found in {}; no AEX filter is registered{}",
        describe_dirs(dirs),
        match scan_limit_note(limits).as_str() {
            "" => " (they were read and hold no .aex)".to_owned(),
            note => note.to_owned(),
        }
    )
}

/// The line for a launch that stopped at the depth cap.
///
/// Warned on its own, and only here: the summaries name the cause but not what
/// it means, and unlike the other two causes this one is not the user's to fix.
/// The cap is sized well past a real install, so reaching it means either a
/// pathological tree or a cap that needs raising again (issue #660). It names the
/// number so whoever reads it knows what to compare against.
fn depth_cap_warning() -> String {
    format!(
        "a folder tree under the scan roots goes deeper than {MAX_SCAN_DEPTH} \
         levels and was not walked to the bottom; any .aex below that is invisible \
         and nothing can be pruned this launch"
    )
}

/// The tail naming why the scan was not authoritative, empty when it was. Each
/// cause carries its own remedy, so nothing binds to the wrong one.
fn scan_limit_note(limits: ScanLimits) -> String {
    match limits.describe_with_remedy() {
        Some(causes) => format!(" ({causes})"),
        None => String::new(),
    }
}

/// The scan folders as one log fragment. Diagnosing a misconfiguration needs the
/// paths themselves, not a count.
fn describe_dirs(dirs: &[PathBuf]) -> String {
    if dirs.is_empty() {
        return "no folder (none configured and no default resolved)".to_owned();
    }
    dirs.iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<String>>()
        .join("; ")
}

/// Sends the load-time counts to the host log.
///
/// Registering nothing while plug-ins *are* known is reported as a warning. That
/// state has several causes that look identical from outside (a worker root that
/// no longer exists, an unbuilt worker, a worker regression failing every
/// plug-in) and used to be silent, so the only symptom was an empty filter list
/// (issues #650, #651).
fn report_registration(known: usize, registered: usize, pending: usize, limits: ScanLimits) {
    let summary = registration_summary(known, registered, pending, limits);
    if registration_is_alarming(known, registered, pending) {
        log_warn(&summary);
    } else {
        log_info(&summary);
    }
}

/// Whether registering nothing is a symptom rather than the expected state.
///
/// A first launch (or one after the cache is deleted) legitimately registers
/// nothing: every plug-in is unknown, so all of them queue for discovery and
/// appear next launch. What is not legitimate is knowing about plug-ins,
/// registering none, and having none queued either — nothing is being shown and
/// nothing is being worked on.
fn registration_is_alarming(known: usize, registered: usize, pending: usize) -> bool {
    known > 0 && registered == 0 && pending == 0
}

/// The load-time counts as one line. Split out so the wording, including the
/// "nothing registered" case, is testable without a host log handle.
fn registration_summary(
    known: usize,
    registered: usize,
    pending: usize,
    limits: ScanLimits,
) -> String {
    // Named whenever it holds, because it changes what the counts mean: `known`
    // then includes cached plug-ins this launch never saw on disk (#321). The
    // cause is named rather than assumed — a tree past the depth cap is not a
    // folder that could not be read, and sends the user somewhere else (#660).
    let scan = match limits.describe() {
        Some(causes) => format!(" ({causes}, so cached plug-ins are included)"),
        None => String::new(),
    };
    if registration_is_alarming(known, registered, pending) {
        return format!(
            "0 of {known} known plug-in(s) registered and none queued for \
             discovery{scan}. Nothing will appear in the filter list: either the \
             compat host worker is failing for every plug-in, or none of them is \
             an effect."
        );
    }
    if registered == 0 && pending > 0 {
        return format!(
            "registered 0 of {known} known plug-in(s); {pending} queued for \
             discovery{scan}. Expected on a first launch or after a host rebuild: \
             they appear once discovery finishes and AviUtl2 is restarted."
        );
    }
    format!(
        "registered {registered} of {known} known plug-in(s); {pending} queued for \
         discovery{scan}"
    )
}

/// Sends the background pass's outcome to the host log.
fn report_discovery(effects: usize, rejected: usize, interrupted: bool, persisted: bool) {
    let summary = discovery_summary(effects, rejected, interrupted);
    if discovery_is_alarming(effects, rejected) {
        log_warn(&summary);
    } else {
        log_info(&summary);
    }
    // The whole point of the pass is what the *next* launch reads back. Without
    // this, a pass that discovered hundreds of effects and could not write any of
    // them still ends on "Restart AviUtl2 to pick them up".
    if !persisted {
        log_warn(
            "the discovery cache could not be written, so this pass will not \
             survive the restart and runs again next launch",
        );
    }
}

/// Whether a discovery pass's outcome is a symptom rather than a normal result.
/// Shared by the wording and the level, so the two cannot drift into an alarming
/// sentence logged at info (or the reverse).
fn discovery_is_alarming(effects: usize, rejected: usize) -> bool {
    effects == 0 && rejected > 0
}

/// The background pass's counts as one line. Split out so the wording is
/// testable without running a discovery.
///
/// Every plug-in being rejected is called out separately. A rejection is normal
/// on its own — a format or codec `.aex` is not an effect — but *nothing*
/// succeeding out of hundreds means the worker itself is failing, which is what
/// issue #651 looked like from the user's side: an empty filter list and no
/// explanation.
///
/// The counts are of discovery *results*, not of cache writes. They differ in two
/// places, both deliberately: a result whose file vanished between discovery and
/// the stat is counted but not written (`keep_best` returns `None`), and a failed
/// re-check of a working entry is counted as rejected while the entry keeps
/// registering (`keep_best` refuses to demote). The question this line answers is
/// "is the worker producing results at all", so the attempt is the right unit.
fn discovery_summary(effects: usize, rejected: usize, interrupted: bool) -> String {
    let tail = if interrupted {
        " (stopped early; the rest is retried next launch)"
    } else {
        ""
    };
    if discovery_is_alarming(effects, rejected) {
        return format!(
            "background discovery: 0 of {rejected} plug-in(s) yielded an \
             effect{tail}. Every one was rejected, so the compat host worker is \
             likely failing rather than the plug-ins being unsupported."
        );
    }
    if effects == 0 {
        return format!("background discovery: nothing was discovered{tail}");
    }
    // "rejected", not "rejected as non-effect": a load failure or a per-plug-in
    // worker fault lands in the same bucket as a format/codec .aex, and this line
    // cannot tell them apart.
    format!(
        "background discovery: {effects} effect(s), {rejected} rejected{tail}. \
         Restart AviUtl2 to pick them up."
    )
}

/// How [`resolve_worker_root`] arrived at its answer, so the plugin can say so
/// rather than leaving a misconfiguration indistinguishable from a healthy
/// launch (issue #655).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WorkerRootSource {
    /// The environment override or the configured `repository`, holding a worker.
    Named,
    /// Shipped beside the plugin.
    BesidePlugin,
    /// A named root with no worker built there, and none beside the plugin.
    NamedWithoutWorker,
}

impl WorkerRootSource {
    /// Names both ways a root can be "named", because the plugin does not record
    /// which one won and sending an env-var user to config.toml wastes their time.
    fn describe(self) -> &'static str {
        match self {
            Self::Named => "from config.toml or AEXCOMPAT_MULTIFILTER_REPOSITORY",
            Self::BesidePlugin => "beside the plugin",
            Self::NamedWithoutWorker => {
                "from config.toml or AEXCOMPAT_MULTIFILTER_REPOSITORY, but no worker \
                 is built there"
            }
        }
    }
}

/// The L2 (discovery) worker, relative to the root handed to the broker. Kept in
/// step with the broker's own `WorkerKind::repository_relative_program`.
const L2_WORKER_RELATIVE_PATH: &str = "target/minihost-build/aex_l2_worker.exe";

/// The path of this running DLL, resolved from an address inside it. Used to
/// fingerprint the in-process broker (its bytes ship in this module, not the
/// worker exe), so a rebuilt-and-redeployed DLL re-verifies the discovery cache.
fn self_module_path() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | _UNCHANGED_REFCOUNT: resolve the
    // module owning `addr` without touching its refcount (no matching FreeLibrary).
    const FROM_ADDRESS_UNCHANGED: u32 = 0x0000_0004 | 0x0000_0002;
    unsafe extern "system" {
        fn GetModuleHandleExW(flags: u32, addr: *const u16, module: *mut isize) -> i32;
        fn GetModuleFileNameW(module: isize, buf: *mut u16, size: u32) -> u32;
    }

    let anchor = self_module_path as *const () as *const u16;
    let mut module: isize = 0;
    // SAFETY: `anchor` points into this module's code; out-params are valid.
    if unsafe { GetModuleHandleExW(FROM_ADDRESS_UNCHANGED, anchor, &mut module) } == 0 {
        return None;
    }
    let mut buf = [0u16; 32768];
    // SAFETY: `module` is a valid HMODULE from the call above; `buf` is sized.
    let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    // 0 = failure; len == buf.len() means truncation (path longer than the buffer).
    if len == 0 || len >= buf.len() {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buf[..len])))
}

fn load_cache() -> HashMap<String, CacheEntry> {
    let Some(path) = cache_path() else {
        return HashMap::new();
    };
    load_cache_at(&path)
}

fn load_cache_at(path: &Path) -> HashMap<String, CacheEntry> {
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    accept_cache_file(serde_json::from_str(&text).unwrap_or_default())
}

/// Only a schema-version mismatch discards entries: the older shape cannot be
/// trusted field-for-field. A host-build change does NOT discard them — each entry
/// carries its own build ([`CacheEntry::build`]) and is re-verified in the
/// background while still being registered, so no filter disappears for a launch
/// (issue #307). An entry that no longer deserializes drops only itself, for the
/// same reason.
fn accept_cache_file(file: CacheFile) -> HashMap<String, CacheEntry> {
    if file.version != CACHE_VERSION {
        return HashMap::new();
    }
    file.entries
        .into_iter()
        .filter_map(|(key, value)| Some((key, serde_json::from_value(value).ok()?)))
        .collect()
}

/// Persists the cache. Returns whether it reached disk: a discovery pass that
/// cannot write has nothing to show on the next launch, and telling the user to
/// restart for it would be a lie (issue #655).
fn save_cache(entries: &HashMap<String, CacheEntry>) -> bool {
    let Some(path) = cache_path() else {
        return false;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // The lock is held across the disk read and the atomic replacement.  A
    // lock around only the final rename would still allow two launches to read
    // the same old cache and lose one another's newly discovered entries.
    let Some(_lock) = acquire_cache_lock(&path) else {
        return false;
    };
    let mut merged_entries = load_cache_at(&path);
    merge_cache_entries(&mut merged_entries, entries);
    let file = CacheFile {
        version: CACHE_VERSION,
        // An entry that cannot be serialized is dropped rather than failing the
        // whole write, so the rest of the cache still survives the launch.
        entries: merged_entries
            .iter()
            .filter_map(|(key, entry)| Some((key.clone(), serde_json::to_value(entry).ok()?)))
            .collect(),
    };
    // Write atomically (temp + rename) so a crash or process exit mid-write (the
    // background thread can still be writing when AviUtl2 quits) never leaves a
    // truncated, unparseable cache file behind. The temp name carries the PID so
    // two AviUtl2 instances do not clobber each other's temp before the rename.
    let Ok(text) = serde_json::to_string(&file) else {
        return false;
    };
    let temp = path.with_file_name(format!("discovery-cache.{}.tmp", std::process::id()));
    if std::fs::write(&temp, text).is_err() {
        return false;
    }
    if std::fs::rename(&temp, &path).is_err() {
        let _ = std::fs::remove_file(&temp);
        return false;
    }
    true
}

/// `(mtime, len)` for cache invalidation; `mtime` degrades to `(0, 0)` if the
/// platform cannot report it (then `len` alone guards, as on the aviutl2 bridge).
fn file_meta(path: &Path) -> Option<((u64, u32), u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0));
    Some((mtime, meta.len()))
}

/// Merges a freshly discovered entry over the cached one, refusing to demote an
/// unchanged AEX from `ok = true` to `ok = false`.
///
/// Re-verification (a host build change) re-runs discovery on AEX that already
/// discovered fine, and discovery has a fixed per-AEX worker deadline, so a
/// transient failure — a timeout under load, with the re-verification pass now
/// competing with the user's own editing and rendering — would otherwise rewrite a
/// working effect as a negative. It would then not be registered on the next
/// launch, and opening a saved project that uses it silently drops those objects
/// for good (issue #307). Keeping the old result instead is the safe direction:
/// if the host really did regress, the effect fails at render time with a
/// diagnostic, which is visible and recoverable, unlike a deleted object.
///
/// A negative only wins when the AEX's current `(mtime, len)` *proves* it changed.
///
/// Returns `None` to mean "leave the cache alone". `meta` is the file's current
/// `(mtime, len)`, or `None` when it could not be stat'd — a transient condition
/// (an AV scanner's sharing violation, a plug-in being replaced). Discovery's own
/// stat can fail the same way, and [`negative_entry`] then falls back to
/// `(0, 0), 0`; writing that out would store an entry under a `(mtime, len)` that
/// never matches the file again, so every later launch would treat the effect as
/// replaced and stop registering it. With no trustworthy meta there is nothing
/// safe to write, so the existing entry (which still registers) is kept and the
/// AEX is re-checked on the next launch.
fn keep_best(
    cached: Option<&CacheEntry>,
    discovered: CacheEntry,
    meta: Option<((u64, u32), u64)>,
) -> Option<CacheEntry> {
    let (mtime, len) = meta?;
    // The AEX differs from what discovery stat'd, so `sha`/`params` may describe
    // the previous bytes. Take the meta just read (so the entry keeps matching the
    // file and stays registered) but mark it for one more pass.
    let stale = discovered.mtime != mtime || discovered.len != len;
    let discovered = CacheEntry {
        mtime,
        len,
        stale,
        ..discovered
    };
    match cached {
        Some(old)
            if old.ok
                && old.stale
                && !discovered.ok
                && old.mtime == mtime
                && old.len == len
                && deterministic_failure(discovered.failure_classification.as_deref()) =>
        {
            // The current bytes were rechecked and failed deterministically.
            // Unlike a timeout, this is safe to converge: keeping the stale
            // payload would register an effect whose SHA no longer opens.
            Some(CacheEntry {
                stale: false,
                ..discovered
            })
        }
        Some(old) if old.ok && !discovered.ok && old.mtime == mtime && old.len == len => {
            Some(CacheEntry {
                // Provenance stays with the host that actually produced the
                // payload; only the attempt is recorded, and it is counted so
                // retries are bounded rather than endless.
                checked: discovered.build,
                attempts: if old.checked == discovered.build {
                    old.attempts.saturating_add(1)
                } else {
                    1
                },
                // Keep any existing stale mark. Clearing it because *this* pass
                // failed would strand an entry whose sha/params describe older
                // bytes: it would never be re-discovered again, so every frame
                // would fail the sha check with no way back except deleting the
                // cache — the very operation that risks issue #307.
                //
                // A stale entry carries the meta just read from disk, so this
                // guard also holds when what is there now genuinely does not
                // discover. Such an entry does not converge on its own while the
                // bytes stay identical: it stays registered on the older bytes'
                // sha, whose session then fails to open, so its frames pass
                // through unrendered — and it is re-discovered every launch.
                // Excluding stale entries here would converge, but at the cost of
                // unregistering one whose re-check merely timed out, trading a
                // fault that leaves the objects in place for the irreversible
                // deletion this path exists to avoid. It also gives up the
                // self-healing: today one later success is enough. Converging
                // safely needs the failure's classification, which is #328.
                stale: old.stale,
                ..old.clone()
            })
        }
        _ => Some(discovered),
    }
}

/// A negative (`ok = false`) cache entry for a plug-in that failed discovery.
fn negative_entry(plugin: &Path, build: BuildFingerprint) -> CacheEntry {
    let (mtime, len) = file_meta(plugin).unwrap_or(((0, 0), 0));
    CacheEntry {
        mtime,
        len,
        ok: false,
        sha: String::new(),
        smart: false,
        params: Vec::new(),
        build,
        stale: false,
        checked: build,
        attempts: 0,
        closure: CachedClosure::default(),
        failure_classification: None,
        alias_fallback: false,
        alias_target: None,
        closure_identity: None,
        cluster_fallback: None,
    }
}

/// Acquires an OS-level exclusive handle on the cache lock file.  The file is
/// intentionally retained after release: unlike a create-new sentinel, a
/// handle lock is released by Windows when the process exits, so a crash cannot
/// strand future saves behind a stale marker.
fn acquire_cache_lock(path: &Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;

    let lock_path = path.with_file_name("discovery-cache.lock");
    for _ in 0..CACHE_LOCK_RETRIES {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .open(&lock_path)
        {
            Ok(lock) => return Some(lock),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                std::thread::sleep(CACHE_LOCK_RETRY);
            }
            Err(_) => return None,
        }
    }
    None
}

/// Unions another launch's cache into this launch's snapshot before replacing
/// the file.  Disjoint AEX results must survive regardless of which launch
/// saves last.  For the same path and file metadata, a known-good entry wins
/// over a negative one so a transient failure cannot erase a usable filter;
/// otherwise the local snapshot remains authoritative for that key.
fn merge_cache_entries(
    local: &mut HashMap<String, CacheEntry>,
    on_disk: &HashMap<String, CacheEntry>,
) {
    for (key, disk_entry) in on_disk {
        match local.get(key) {
            None => {
                local.insert(key.clone(), disk_entry.clone());
            }
            Some(local_entry)
                if disk_entry.ok
                    && !local_entry.ok
                    && disk_entry.mtime == local_entry.mtime
                    && disk_entry.len == local_entry.len =>
            {
                local.insert(key.clone(), disk_entry.clone());
            }
            Some(_) => {}
        }
    }
}

/// Extract the broker's already-normalized worker classification from the
/// diagnostic JSON embedded in an inspection error. Missing or malformed
/// diagnostics stay unknown and therefore retain the old safe behavior.
fn inspection_failure_classification(error: &std::io::Error) -> Option<String> {
    let message = error.to_string();
    let payload = message.split_once("diagnostics=")?.1;
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()?
        .get("classification")?
        .as_str()
        .map(str::to_owned)
}

fn deterministic_failure(classification: Option<&str>) -> bool {
    matches!(classification, Some("nonzero_exit" | "crashed"))
}

/// The closure identity (issue #405): a normalized hash of the resolved
/// dependency set — sorted `basename:sha256` pairs — so two plug-ins hash to
/// the same identity exactly when their closures carry the same modules.
/// Case-folding the basename matches the loader's own collision rules.
fn closure_identity_of(dependencies: &[ApprovedImageArtifact]) -> String {
    let mut entries: Vec<String> = dependencies
        .iter()
        .map(|dependency| {
            let basename = dependency
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_lowercase();
            format!("{}:{}", basename, hex_lower(&dependency.expected_sha256))
        })
        .collect();
    entries.sort();
    hex_lower(&Sha256::digest(entries.join("\n").as_bytes()))
}
