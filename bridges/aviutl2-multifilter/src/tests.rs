#[cfg(test)]
mod tests {
    use super::*;

    const META: Option<((u64, u32), u64)> = Some(((5, 0), 64));
    /// The AEX could not be stat'd this pass (transient: AV scanner, replacement).
    const NO_META: Option<((u64, u32), u64)> = None;

    fn build(worker_mtime: u64) -> BuildFingerprint {
        BuildFingerprint {
            worker: Some((worker_mtime, 0, 4096)),
            host: Some((100, 0, 8192)),
            dependency_inputs: 0,
        }
    }

    fn discovered(mtime_secs: u64, len: u64, build: BuildFingerprint) -> CacheEntry {
        CacheEntry {
            mtime: (mtime_secs, 0),
            len,
            ok: true,
            sha: "aa".into(),
            smart: true,
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

    fn failed(mtime_secs: u64, len: u64, build: BuildFingerprint) -> CacheEntry {
        CacheEntry {
            ok: false,
            sha: String::new(),
            smart: false,
            ..discovered(mtime_secs, len, build)
        }
    }

    // --- keep_best: never lose a working effect to a transient failure -------

    /// The core of issue #307: a transient re-verification failure (a worker
    /// timeout under load) must not turn a working effect into a negative, or the
    /// next launch stops registering it and a saved project silently loses every
    /// object that used it.
    #[test]
    fn a_failed_reverification_does_not_demote_an_unchanged_effect() {
        let old = discovered(5, 64, build(1));
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META).unwrap();
        assert!(merged.ok, "an unchanged, previously working AEX stayed ok");
        assert_eq!(merged.sha, "aa", "the old payload was kept");
        assert!(merged.smart);
    }

    /// A failure to stat the AEX is not evidence that it changed, so it must not
    /// open the demotion path either.
    /// With no trustworthy meta there is nothing safe to write: discovery's own
    /// stat may have failed too, and storing its `(0, 0), 0` fallback would make
    /// every later launch see a mismatch and unregister the effect (#307).
    #[test]
    fn an_unreadable_aex_leaves_the_cache_alone() {
        let old = discovered(5, 64, build(1));
        assert!(keep_best(Some(&old), failed(0, 0, build(2)), NO_META).is_none());
        assert!(
            keep_best(None, discovered(0, 0, build(2)), NO_META).is_none(),
            "a successful discovery with no meta is not written either"
        );
    }

    /// A failed re-verification must not pass the payload off as the current
    /// host's work: that hides which host produced it and ends re-verification
    /// for that host, stranding the effect on older parameters after a single
    /// transient failure. The attempt is recorded separately and retried.
    #[test]
    fn a_failed_recheck_does_not_claim_the_new_build() {
        let old = discovered(5, 64, build(1));
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META).unwrap();
        assert_eq!(merged.build, build(1), "provenance is unchanged");
        assert_eq!(merged.checked, build(2), "but the attempt is recorded");
        assert_eq!(merged.attempts, 1);
        assert_eq!(
            classify(Some(&merged), META, build(2)),
            LoadDecision {
                register: true,
                discover: true
            },
            "still registered, and tried again"
        );
    }

    /// Retries are bounded, so a host that genuinely cannot discover an effect
    /// any more does not re-run a worker for it on every launch forever.
    #[test]
    fn re_verification_gives_up_after_the_retry_budget() {
        let mut entry = discovered(5, 64, build(1));
        for attempt in 1..=RETRY_BUDGET {
            entry = keep_best(Some(&entry), failed(5, 64, build(2)), META).unwrap();
            assert_eq!(entry.attempts, attempt);
            assert!(entry.ok, "registered throughout");
        }
        assert_eq!(
            classify(Some(&entry), META, build(2)),
            LoadDecision {
                register: true,
                discover: false
            },
            "converged: still registered, no longer queued"
        );
        // A different host starts the budget over, since it may well succeed.
        assert_eq!(
            classify(Some(&entry), META, build(3)),
            LoadDecision {
                register: true,
                discover: true
            }
        );
    }

    /// A replaced AEX is a different plug-in, so its old parameters are
    /// meaningless and the negative result must win.
    #[test]
    fn a_replaced_aex_may_become_negative() {
        let old = discovered(5, 64, build(1));
        let newer = Some(((9, 0), 64));
        let resized = Some(((5, 0), 99));
        assert!(
            !keep_best(Some(&old), failed(9, 64, build(1)), newer)
                .unwrap()
                .ok
        );
        assert!(
            !keep_best(Some(&old), failed(5, 99, build(1)), resized)
                .unwrap()
                .ok
        );
    }

    /// The point of re-verifying at all (issue #304): a host that gained support
    /// for an effect promotes the old negative.
    #[test]
    fn a_new_host_promotes_a_previously_failing_effect() {
        let old = failed(5, 64, build(1));
        let merged = keep_best(Some(&old), discovered(5, 64, build(2)), META).unwrap();
        assert!(merged.ok);
        assert_eq!(merged.build, build(2));
    }

    #[test]
    fn a_first_discovery_is_taken_as_is() {
        assert!(!keep_best(None, failed(5, 64, build(1)), META).unwrap().ok);
        assert!(
            keep_best(None, discovered(5, 64, build(1)), META)
                .unwrap()
                .ok
        );
    }

    /// Discovery records the AEX's meta itself, and the file can change between
    /// that stat and the read (or that stat can fail, falling back to `(0, 0), 0`).
    /// Storing the entry under a `(mtime, len)` that never matches again would
    /// make every later launch unregister it, so the merge stamps the meta it read
    /// — and marks the entry stale, because `sha`/`params` may describe the older
    /// bytes and would otherwise stay wrong forever without being re-discovered.
    #[test]
    fn a_discovery_that_raced_the_file_is_stamped_and_marked_stale() {
        let fresh = discovered(9, 99, build(1)); // discovery saw different bytes
        let merged = keep_best(None, fresh, META).unwrap();
        assert_eq!(merged.mtime, (5, 0), "matches the file, so it registers");
        assert_eq!(merged.len, 64);
        assert!(merged.stale);
        assert_eq!(
            classify(Some(&merged), META, build(1)),
            LoadDecision {
                register: true,
                discover: true
            },
            "registered (no object loss) and re-discovered (self-heals)"
        );
    }

    /// The ordinary case must not be marked stale, or every entry re-discovers
    /// on every launch.
    #[test]
    fn an_undisturbed_discovery_is_not_stale() {
        let merged = keep_best(None, discovered(5, 64, build(1)), META).unwrap();
        assert!(!merged.stale);
        assert_eq!(
            classify(Some(&merged), META, build(1)),
            LoadDecision {
                register: true,
                discover: false
            }
        );
    }

    // --- classify: an older host must not unregister a filter ---------------

    /// The other half of issue #307: an entry from an older host keeps being
    /// registered while it is re-verified, instead of vanishing for a launch.
    #[test]
    fn an_entry_from_an_older_host_is_registered_and_reverified() {
        let old = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&old), META, build(2)),
            LoadDecision {
                register: true,
                discover: true
            }
        );
    }

    #[test]
    fn a_current_entry_is_registered_without_rediscovery() {
        let entry = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), META, build(1)),
            LoadDecision {
                register: true,
                discover: false
            }
        );
    }

    /// A cached non-effect (a format/codec `.aex`) is not registered, and is only
    /// re-probed when the host changed.
    #[test]
    fn a_cached_negative_is_not_registered() {
        let entry = failed(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), META, build(1)),
            LoadDecision {
                register: false,
                discover: false
            }
        );
        assert_eq!(
            classify(Some(&entry), META, build(2)),
            LoadDecision {
                register: false,
                discover: true
            }
        );
    }

    /// A stat failure on a path the scan just found is not evidence the AEX
    /// changed, so the cached result keeps being registered. Unregistering it for
    /// this launch would delete objects from saved projects that use it (#307).
    #[test]
    fn an_unstattable_aex_stays_registered() {
        let entry = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), NO_META, build(1)),
            LoadDecision {
                register: true,
                discover: true
            },
            "registered from cache, and re-checked in the background"
        );
    }

    /// ...but an unknown AEX with no cached entry still has nothing to register.
    #[test]
    fn an_unstattable_aex_without_a_cache_entry_is_only_discovered() {
        assert_eq!(
            classify(None, NO_META, build(1)),
            LoadDecision {
                register: false,
                discover: true
            }
        );
    }

    /// A cached negative is not resurrected by a stat failure.
    #[test]
    fn an_unstattable_negative_is_still_not_registered() {
        let entry = failed(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), NO_META, build(1)),
            LoadDecision {
                register: false,
                discover: true
            }
        );
    }

    #[test]
    fn an_unknown_aex_is_only_discovered() {
        assert_eq!(
            classify(None, META, build(1)),
            LoadDecision {
                register: false,
                discover: true
            },
            "never seen"
        );
    }

    /// A replacement must remain visible for this launch. Its cached payload
    /// may fail the SHA check, but keeping the filter registered prevents
    /// AviUtl2 from deleting objects before background discovery replaces the
    /// entry (#309).
    #[test]
    fn a_replaced_aex_keeps_a_known_good_registration_until_rediscovered() {
        let entry = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), Some(((9, 0), 64)), build(1)),
            LoadDecision {
                register: true,
                discover: true
            },
            "keep the last known-good filter registered while the replacement is discovered"
        );
    }

    // --- prune: never conclude "gone" from an incomplete scan ---------------

    fn json_entries(keys: &[&str]) -> HashMap<String, serde_json::Value> {
        cache_of(keys)
            .iter()
            .map(|(key, entry)| (key.clone(), serde_json::to_value(entry).unwrap()))
            .collect()
    }

    fn cache_of(keys: &[&str]) -> HashMap<String, CacheEntry> {
        keys.iter()
            .map(|key| ((*key).to_string(), discovered(5, 64, build(1))))
            .collect()
    }

    #[test]
    fn a_complete_scan_prunes_entries_whose_aex_is_gone() {
        let mut cache = cache_of(&["a.aex", "gone.aex"]);
        prune_cache(
            &mut cache,
            &[PathBuf::from("a.aex")],
            &[PathBuf::from("")],
            true,
        );
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key("a.aex"));
    }

    /// A folder that could not be read (or a default folder that went missing)
    /// must not make its effects look deleted: pruning them would leave them
    /// unregistered next launch and delete objects from saved projects (#307).
    #[test]
    fn an_incomplete_scan_prunes_nothing() {
        let mut cache = cache_of(&["a.aex", "unscanned.aex"]);
        prune_cache(
            &mut cache,
            &[PathBuf::from("a.aex")],
            &[PathBuf::from("")],
            false,
        );
        assert_eq!(cache.len(), 2, "the unscanned entry survived");
    }

    #[test]
    fn an_incomplete_scan_registers_cached_entries_under_its_roots() {
        let root = PathBuf::from("scan-root");
        let cached = root.join("temporarily-hidden.aex");
        let outside = PathBuf::from("other-root").join("outside.aex");
        let cache = cache_of(&[&cached.to_string_lossy(), &outside.to_string_lossy()]);
        let fallback = cached_fallback_plugins(
            &cache,
            &[root.join("visible.aex")],
            std::slice::from_ref(&root),
            false,
            false,
            &[],
        );
        assert_eq!(fallback, vec![cached]);
    }

    #[test]
    fn missing_scan_roots_keep_all_registerable_cached_entries() {
        let first_root = PathBuf::from("first-root");
        let first = first_root.join("first.aex");
        let second = PathBuf::from("second-root").join("second.aex");
        let cache = cache_of(&[&first.to_string_lossy(), &second.to_string_lossy()]);
        let fallback = cached_fallback_plugins(
            &cache,
            &[],
            std::slice::from_ref(&first_root),
            false,
            true,
            &[],
        );
        assert_eq!(fallback.len(), 2);
        assert!(fallback.contains(&first));
        assert!(fallback.contains(&second));
    }

    #[test]
    fn complete_scan_does_not_resurrect_missing_cached_entries() {
        let root = PathBuf::from("scan-root");
        let cached = root.join("gone.aex");
        let cache = cache_of(&[&cached.to_string_lossy()]);
        assert!(
            cached_fallback_plugins(
                &cache,
                &[root.join("visible.aex")],
                std::slice::from_ref(&root),
                true,
                false,
                &[],
            )
            .is_empty()
        );
    }

    // --- cache file acceptance ----------------------------------------------

    /// Entries must survive being read back; only a schema-version change
    /// discards them (a host-build change is handled per entry).
    #[test]
    fn a_current_cache_file_keeps_its_entries() {
        let file = CacheFile {
            version: CACHE_VERSION,
            entries: json_entries(&["a.aex"]),
        };
        assert_eq!(accept_cache_file(file).len(), 1);
    }

    #[test]
    fn a_future_or_older_schema_is_discarded() {
        let file = CacheFile {
            version: CACHE_VERSION + 1,
            entries: json_entries(&["a.aex"]),
        };
        assert!(accept_cache_file(file).is_empty());
    }

    #[test]
    fn a_concurrent_cache_save_preserves_disjoint_discoveries() {
        let mut local = cache_of(&["local.aex"]);
        let on_disk = cache_of(&["other-process.aex"]);

        merge_cache_entries(&mut local, &on_disk);

        assert!(local.contains_key("local.aex"));
        assert!(local.contains_key("other-process.aex"));
    }

    #[test]
    fn a_concurrent_cache_save_keeps_known_good_for_an_unchanged_aex() {
        let key = "same.aex";
        let mut local = HashMap::from([(key.to_string(), failed(5, 64, build(1)))]);
        let on_disk = HashMap::from([(key.to_string(), discovered(5, 64, build(1)))]);

        merge_cache_entries(&mut local, &on_disk);

        assert!(
            local[key].ok,
            "a transient negative must not erase a good entry"
        );
    }

    #[test]
    fn a_concurrent_cache_save_keeps_current_negative_for_changed_bytes() {
        let key = "changed.aex";
        let mut local = HashMap::from([(key.to_string(), failed(9, 64, build(1)))]);
        let on_disk = HashMap::from([(key.to_string(), discovered(5, 64, build(1)))]);

        merge_cache_entries(&mut local, &on_disk);

        assert!(
            !local[key].ok,
            "a result for older bytes must not be resurrected"
        );
    }

    // --- scan completeness ---------------------------------------------------

    #[test]
    fn a_readable_folder_scans_completely() {
        let dir = std::env::temp_dir().join(format!("aexcompat-mf-{}-scan", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let complete = collect_aex(&[dir], &[]).complete;
        assert!(complete);
    }

    #[test]
    fn an_unreadable_folder_marks_the_scan_incomplete() {
        let missing =
            std::env::temp_dir().join(format!("aexcompat-mf-{}-missing", std::process::id()));
        let scan = collect_aex(&[missing], &[]);
        assert!(scan.plugins.is_empty());
        assert!(
            !scan.complete,
            "a folder that could not be read is not a complete scan"
        );
    }

    // --- default folder resolution ------------------------------------------

    /// A temp dir unique to this test and this process, so concurrent `cargo test`
    /// runs do not delete each other's fixtures.
    /// An entry whose closure was resolved against `roots` and sealed `sealed`.
    fn with_closure(roots: &[&Path], sealed: &[&Path], missing: &[&str]) -> CacheEntry {
        let mut entry = discovered(5, 64, build(1));
        entry.closure = CachedClosure {
            roots: roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect(),
            sealed: sealed
                .iter()
                .map(|path| {
                    let (mtime, len) = file_meta(path).expect("sealed dependency");
                    CachedDependency {
                        path: path.to_string_lossy().into_owned(),
                        mtime,
                        len,
                    }
                })
                .collect(),
            missing: missing.iter().map(|name| name.to_string()).collect(),
            provenance: Vec::new(),
        };
        entry
    }

    #[test]
    fn an_unchanged_closure_is_not_re_discovered() {
        let root = temp_root("closure-stable");
        let dependency = root.join("dvacore.dll");
        std::fs::write(&dependency, b"runtime").unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let dependency = std::fs::canonicalize(&dependency).unwrap();

        let entry = with_closure(&[&root], &[&dependency], &["kernel32.dll"]);
        assert!(!needs_closure_recheck(&entry, build(1), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rewritten_or_removed_dependency_re_discovers_that_effect() {
        let root = temp_root("closure-rewritten");
        let dependency = root.join("dvacore.dll");
        std::fs::write(&dependency, b"runtime").unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let dependency = std::fs::canonicalize(&dependency).unwrap();
        let entry = with_closure(&[&root], &[&dependency], &[]);

        // An AE update rewriting the DLL in place changes neither the AEX nor the
        // host build, so nothing else would notice it.
        std::fs::write(&dependency, b"a different runtime").unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));

        std::fs::remove_file(&dependency).unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_import_that_appears_re_discovers_the_effect_that_wanted_it() {
        let root = temp_root("closure-appeared");
        let root = std::fs::canonicalize(&root).unwrap();
        let entry = with_closure(&[&root], &[], &["helper.dll"]);
        assert!(!needs_closure_recheck(&entry, build(1), &[root.clone()]));

        // The missing dependency turns up: the plug-in that failed for want of it
        // is exactly the one to try again.
        std::fs::write(root.join("helper.dll"), b"helper").unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_entry_with_no_recorded_closure_is_re_verified_once() {
        // Written before #304: nothing recorded, so it cannot be judged unchanged.
        // It is re-verified — and, per issue #307, stays registered meanwhile.
        let root = temp_root("closure-legacy");
        let root = std::fs::canonicalize(&root).unwrap();
        let entry = discovered(5, 64, build(1));
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));
        assert!(classify(Some(&entry), META, build(1)).register);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_closure_recheck_gives_up_after_the_retry_budget() {
        // Otherwise one rewritten runtime DLL re-runs a worker for every effect on
        // every launch: the re-discovery fails, `keep_best` keeps the old entry
        // and its old record, and the trigger fires again unchanged.
        let root = temp_root("closure-budget");
        let root = std::fs::canonicalize(&root).unwrap();
        let mut entry = with_closure(&[&root], &[], &["helper.dll"]);
        std::fs::write(root.join("helper.dll"), b"helper").unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));

        entry.checked = build(1);
        entry.attempts = RETRY_BUDGET;
        assert!(!needs_closure_recheck(&entry, build(1), &[root.clone()]));
        // A different host gets its own budget.
        assert!(needs_closure_recheck(&entry, build(2), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aexcompat-mf-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp root");
        dir
    }

    #[test]
    fn the_newest_versioned_install_wins() {
        let root = temp_root("newest");
        for version in ["2024", "2025"] {
            std::fs::create_dir_all(root.join(format!("App {version}")).join("Plug-ins")).unwrap();
        }
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert!(complete);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
    }

    /// An install being updated has its version folder but not yet its leaf.
    /// Falling back to the older version must not also claim the scan was
    /// complete, or the newer version's effects get pruned and unregistered (#307).
    #[test]
    fn a_version_missing_its_leaf_marks_the_resolution_incomplete() {
        let root = temp_root("updating");
        std::fs::create_dir_all(root.join("App 2024").join("Plug-ins")).unwrap();
        std::fs::create_dir_all(root.join("App 2025")).unwrap(); // mid-update
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2024").join("Plug-ins"));
        assert!(!complete, "fell back to an older version, so not complete");
    }

    /// Unversioned clutter next to the installs is not evidence of anything.
    #[test]
    fn unversioned_entries_do_not_mark_the_resolution_incomplete() {
        let root = temp_root("clutter");
        std::fs::create_dir_all(root.join("App 2025").join("Plug-ins")).unwrap();
        std::fs::write(root.join("App readme.txt"), b"x").unwrap();
        std::fs::create_dir_all(root.join("App Common")).unwrap();
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
        assert!(complete);
    }

    #[test]
    fn a_missing_root_is_incomplete() {
        let root =
            std::env::temp_dir().join(format!("aexcompat-mf-{}-no-root", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert!(picked.is_none());
        assert!(!complete);
    }

    /// The cache file is shared across scan configurations, so a launch that
    /// scanned only one folder must not conclude the other folders' effects are
    /// gone — that would unregister them all on the next normal launch (#307).
    #[test]
    fn a_prune_only_judges_the_folders_it_scanned() {
        let scanned = PathBuf::from("scan-root");
        let inside = scanned.join("gone.aex").to_string_lossy().into_owned();
        let outside = PathBuf::from("other-root")
            .join("kept.aex")
            .to_string_lossy()
            .into_owned();
        let mut cache = cache_of(&[&inside, &outside]);
        prune_cache(&mut cache, &[], &[scanned], true);
        assert!(!cache.contains_key(&inside), "scanned and absent: pruned");
        assert!(
            cache.contains_key(&outside),
            "outside the scanned roots: untouched"
        );
    }

    /// `CacheEntry::params` embeds a broker type whose fields are not all
    /// defaulted. One entry that no longer parses must not empty the cache and
    /// unregister every filter for a launch (#307).
    #[test]
    fn one_unparseable_entry_does_not_discard_the_rest() {
        let mut entries = json_entries(&["good.aex"]);
        entries.insert(
            "broken.aex".into(),
            serde_json::json!({"mtime": "not-a-tuple"}),
        );
        let accepted = accept_cache_file(CacheFile {
            version: CACHE_VERSION,
            entries,
        });
        assert_eq!(accepted.len(), 1);
        assert!(accepted.contains_key("good.aex"));
    }

    /// A host fingerprint that could not be read is not "a different host":
    /// treating it as one re-discovers everything twice for one failed stat.
    #[test]
    fn an_unknown_host_build_does_not_force_rediscovery() {
        let entry = discovered(5, 64, build(1));
        let unknown = BuildFingerprint::default();
        assert!(!unknown.is_known());
        assert_eq!(
            classify(Some(&entry), META, unknown),
            LoadDecision {
                register: true,
                discover: false
            }
        );
    }

    /// An uninstall leaves empty version folders behind; one older than the pick
    /// says nothing about the install being incomplete.
    #[test]
    fn a_leafless_older_version_does_not_mark_the_resolution_incomplete() {
        let root = temp_root("leftover");
        std::fs::create_dir_all(root.join("App 2025").join("Plug-ins")).unwrap();
        std::fs::create_dir_all(root.join("App 2019")).unwrap(); // uninstall leftover
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
        assert!(complete);
    }

    /// The MediaCore shape: no prefix, so every entry under the root is a
    /// candidate.
    #[test]
    fn an_empty_prefix_picks_the_newest_bare_version() {
        let root = temp_root("mediacore");
        for version in ["7.0", "10.0", "CS6"] {
            std::fs::create_dir_all(root.join(version).join("MediaCore")).unwrap();
        }
        let (picked, complete) = newest_versioned(&root, "", &["MediaCore"]);
        assert_eq!(picked.unwrap(), root.join("10.0").join("MediaCore"));
        assert!(complete);
    }

    /// A re-verification that failed must not clear the stale mark: the entry's
    /// sha/params would stay wrong forever with nothing left to re-discover it,
    /// so every frame would fail the sha check (issue #307's recovery step is the
    /// cache deletion that itself risks the data loss).
    #[test]
    fn a_failed_recheck_keeps_an_existing_stale_mark() {
        let mut old = discovered(5, 64, build(1));
        old.stale = true;
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META).unwrap();
        assert!(merged.ok, "still registered");
        assert!(merged.stale, "still queued for another attempt");
        assert_eq!(
            classify(Some(&merged), META, build(2)),
            LoadDecision {
                register: true,
                discover: true
            }
        );
    }

    /// An ignored AEX is still on disk. Pruning its entry would leave it
    /// unregistered on the launch after it is taken back out of `ignore` (#307).
    #[test]
    fn an_ignored_aex_keeps_its_cache_entry() {
        let root = temp_root("ignored");
        std::fs::write(root.join("keep.aex"), b"x").unwrap();
        std::fs::write(root.join("skip.aex"), b"x").unwrap();
        let scan = collect_aex(std::slice::from_ref(&root), &["skip".into()]);
        assert_eq!(scan.plugins.len(), 1, "the ignored one is not registered");
        assert_eq!(scan.seen.len(), 2, "but it was seen");

        let key = root.join("skip.aex").to_string_lossy().into_owned();
        let mut cache = cache_of(&[&key]);
        prune_cache(&mut cache, &scan.seen, &[root], true);
        assert!(cache.contains_key(&key), "an ignored AEX is not gone");
    }

    /// A stray file whose name carries digits is not an install, and must not
    /// look like one missing its leaf — that would disable the prune forever.
    #[test]
    fn a_stray_file_is_not_a_version() {
        let root = temp_root("stray-file");
        std::fs::create_dir_all(root.join("App 2025").join("Plug-ins")).unwrap();
        std::fs::write(root.join("App 2026.log"), b"x").unwrap();
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
        assert!(complete);
    }

    #[test]
    fn dependency_provenance_is_additive_and_round_trips() {
        let closure = CachedClosure {
            provenance: vec![CachedDependencyProvenance {
                basename: "runtime.dll".into(),
                import_derived: false,
                string_derived: true,
            }],
            ..CachedClosure::default()
        };
        let encoded = serde_json::to_value(&closure).unwrap();
        assert_eq!(encoded["provenance"][0]["basename"], "runtime.dll");
        assert_eq!(encoded["provenance"][0]["string_derived"], true);
        let decoded: CachedClosure = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.provenance.len(), 1);
        assert!(!decoded.provenance[0].import_derived);
        assert!(decoded.provenance[0].string_derived);
    }

    /// The cache embeds the broker's `InteractiveParameter`, whose fields are not
    /// all defaulted, so a field added there stops every entry that has
    /// parameters from deserializing at once — every registerable filter, on
    /// users' machines, with the object deletion of issue #307 behind it. Pin the
    /// shape here so that change fails in `cargo test` instead.
    ///
    /// If this fails because the broker type gained a field: give the new field
    /// `#[serde(default)]` there (so old caches still read), then add it here.
    #[test]
    fn the_cached_parameter_schema_is_stable() {
        let frozen = serde_json::json!({
            "slot": 0,
            "name": "Intensity",
            "kind": "float",
            "minimum": 0.0,
            "maximum": 100.0,
            "value": 50.0,
            "choices": [],
            "color": [0, 0, 0, 255],
            "components": [0.0, 0.0, 0.0],
            "component_count": 0,
            "layer_path": null,
            "enabled": true,
            "visible": true,
            "supervised": false,
        });
        let parsed = serde_json::from_value::<InteractiveParameter>(frozen.clone());
        assert!(
            parsed.is_ok(),
            "a cache written by an older build no longer deserializes: {:?}",
            parsed.err()
        );
        // And the fields we persist still round-trip.
        let value = serde_json::to_value(parsed.unwrap()).unwrap();
        for key in frozen.as_object().unwrap().keys() {
            assert!(
                value.get(key).is_some(),
                "field `{key}` disappeared from the schema"
            );
        }
    }

    #[test]
    fn non_finite_parameters_are_normalized_before_cache_round_trip() {
        let mut parameters = vec![InteractiveParameter {
            slot: 1,
            name: "Broken range".into(),
            kind: "float".into(),
            minimum: f64::NAN,
            maximum: f64::INFINITY,
            value: f64::NEG_INFINITY,
            choices: Vec::new(),
            color: [0, 0, 0, 255],
            components: [f64::NAN, 0.5, f64::INFINITY],
            component_count: 3,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }];

        normalize_parameters_for_cache(&mut parameters);
        let parameter = &parameters[0];
        assert_eq!(
            (parameter.minimum, parameter.maximum, parameter.value),
            (0.0, 1.0, 0.0)
        );
        assert_eq!(parameter.components, [0.0, 0.5, 0.0]);

        let encoded = serde_json::to_value(&parameters).expect("finite parameters serialize");
        let decoded: Vec<InteractiveParameter> =
            serde_json::from_value(encoded).expect("normalized parameters deserialize");
        assert_eq!(decoded[0].value, 0.0);
        assert!(decoded[0].components.iter().all(|value| value.is_finite()));
    }

    /// The same hazard for this crate's own entry shape: adding a field without
    /// `#[serde(default)]` stops every existing cache entry from deserializing.
    #[test]
    fn an_older_cache_entry_shape_still_reads() {
        // What an entry written before `params`/`build`/`stale` existed looks like.
        let oldest = serde_json::json!({
            "mtime": [5, 0],
            "len": 64,
            "ok": true,
            "sha": "aa",
            "smart": true,
        });
        let entry: CacheEntry = serde_json::from_value(oldest)
            .expect("an entry from an older build must still read, or every filter unregisters");
        assert!(entry.ok);
        assert!(entry.params.is_empty());
        assert!(!entry.stale);
        assert_eq!(entry.build, BuildFingerprint::default());
    }

    /// A junctioned install (Adobe moved to another drive) must still be found:
    /// `DirEntry::file_type` calls a junction a symlink, not a directory.
    #[cfg(windows)]
    #[test]
    fn a_junctioned_install_is_still_found() {
        let root = temp_root("junction");
        let real = root.join("real");
        std::fs::create_dir_all(real.join("Plug-ins")).unwrap();
        let link = root.join("App 2025");
        junction(&link, &real);
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), link.join("Plug-ins"));
        assert!(complete);
    }

    /// The same for a junctioned subfolder of a scan root: missing it would leave
    /// its AEX out of `seen`, and the prune would then delete their entries.
    #[cfg(windows)]
    #[test]
    fn a_junctioned_subfolder_is_scanned() {
        let root = temp_root("junction-scan");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("deep.aex"), b"x").unwrap();
        let scanned = root.join("scanned");
        std::fs::create_dir_all(&scanned).unwrap();
        junction(&scanned.join("linked"), &real);
        let scan = collect_aex(std::slice::from_ref(&scanned), &[]);
        assert_eq!(scan.seen.len(), 1, "the AEX behind the junction was seen");
        assert!(scan.complete);
    }

    /// Creates a directory junction, failing loudly rather than letting the test
    /// pass without exercising anything. Junctions need no elevation on NTFS.
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("run mklink");
        assert!(
            out.status.success(),
            "could not create a junction, so this test proves nothing: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A junction whose target is not mounted this launch hides whatever is
    /// behind it. Reporting the scan as complete would let the prune delete those
    /// AEX's cache entries, unregistering them once the drive is back (#307).
    #[cfg(windows)]
    #[test]
    fn an_unresolvable_junction_marks_the_scan_incomplete() {
        let root = temp_root("dangling");
        let target = root.join("target");
        std::fs::create_dir_all(target.join("sub")).unwrap();
        let scanned = root.join("scanned");
        std::fs::create_dir_all(&scanned).unwrap();
        junction(&scanned.join("linked"), &target);
        std::fs::remove_dir_all(&target).unwrap(); // the drive went away

        let scan = collect_aex(std::slice::from_ref(&scanned), &[]);
        assert!(
            !scan.complete,
            "an unresolvable link is 'not looked at', not 'nothing there'"
        );

        let hidden = scanned
            .join("linked")
            .join("deep.aex")
            .to_string_lossy()
            .into_owned();
        let mut cache = cache_of(&[&hidden]);
        prune_cache(&mut cache, &scan.seen, &[scanned], scan.complete);
        assert!(cache.contains_key(&hidden), "its entry survived");
    }

    /// The same for a version folder that is an unresolvable junction: falling
    /// back to an older version must not also claim the resolution was complete.
    #[cfg(windows)]
    #[test]
    fn an_unresolvable_version_junction_marks_the_resolution_incomplete() {
        let root = temp_root("dangling-version");
        std::fs::create_dir_all(root.join("App 2024").join("Plug-ins")).unwrap();
        let target = root.join("target");
        std::fs::create_dir_all(&target).unwrap();
        junction(&root.join("App 2025"), &target);
        std::fs::remove_dir_all(&target).unwrap();

        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2024").join("Plug-ins"));
        assert!(!complete, "the newer install was not visible, not absent");
    }

    /// A junction pointing back up the tree must not expose the same AEX as a
    /// pile of duplicate filters (each with its own discovery worker).
    #[cfg(windows)]
    #[test]
    fn a_junction_loop_does_not_duplicate_an_aex() {
        let root = temp_root("junction-loop");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("only.aex"), b"x").unwrap();
        junction(&root.join("loop"), &root);
        let scan = collect_aex(std::slice::from_ref(&root), &[]);
        assert_eq!(scan.seen.len(), 1, "one AEX, seen once: {:?}", scan.seen);
    }

    /// Two scan roots that reach the same folder through a junction likewise
    /// must not register everything under it twice.
    #[cfg(windows)]
    #[test]
    fn two_roots_crossing_through_a_junction_do_not_duplicate() {
        let root = temp_root("junction-cross");
        let shared = root.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("one.aex"), b"x").unwrap();
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        junction(&other.join("link"), &shared);
        let scan = collect_aex(&[shared.clone(), other], &[]);
        assert_eq!(scan.seen.len(), 1, "one AEX, seen once: {:?}", scan.seen);
        assert!(scan.complete, "reaching it twice is not an incomplete scan");
    }

    /// The scan lists one spelling per AEX, so an entry keyed by another path to
    /// the same file (reached through a junction) is missing from the listing but
    /// is not gone. Pruning it would unregister that filter next launch (#307).
    #[cfg(windows)]
    #[test]
    fn an_entry_reachable_under_another_name_is_not_pruned() {
        let root = temp_root("alias");
        let real = root.join("Effects");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("AAA-link"), &real);

        let scan = collect_aex(std::slice::from_ref(&root), &[]);
        assert_eq!(scan.seen.len(), 1, "walked once: {:?}", scan.seen);

        // Key the cache by the spelling the scan did NOT keep.
        let other = if scan.seen[0].starts_with(&real) {
            root.join("AAA-link").join("foo.aex")
        } else {
            real.join("foo.aex")
        };
        let key = other.to_string_lossy().into_owned();
        let mut cache = cache_of(&[&key]);
        prune_cache(&mut cache, &scan.seen, &[root], scan.complete);
        assert!(
            cache.contains_key(&key),
            "the file is still there, so is its entry"
        );
    }

    /// An AEX that really is gone still goes, or the cache never shrinks.
    #[test]
    fn a_deleted_aex_is_still_pruned() {
        let root = temp_root("deleted");
        let key = root.join("gone.aex").to_string_lossy().into_owned();
        let mut cache = cache_of(&[&key]);
        prune_cache(&mut cache, &[], &[root], true);
        assert!(
            cache.is_empty(),
            "the file does not exist, so the entry goes"
        );
    }

    /// The spelling the scan walks can change between launches (a junction added
    /// or renamed, a different scan-root order) without the file changing. The
    /// entry must still be found, or that effect is unregistered for a launch and
    /// saved projects lose the objects using it (#307).
    #[cfg(windows)]
    #[test]
    fn an_entry_keyed_under_another_name_is_still_found() {
        let root = temp_root("alias-lookup");
        let real = root.join("Effects");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("AAA-link"), &real);

        let scan = collect_aex(std::slice::from_ref(&root), &[]);
        assert_eq!(scan.seen.len(), 1);
        let walked = &scan.seen[0];
        // Key the cache by the other spelling, as an earlier launch would have.
        let other = if walked.starts_with(&real) {
            root.join("AAA-link").join("foo.aex")
        } else {
            real.join("foo.aex")
        };
        let cache = cache_of(&[&other.to_string_lossy()]);

        assert!(
            !cache.contains_key(&walked.to_string_lossy().into_owned()),
            "the exact key really does miss"
        );
        let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
        let found = walked
            .canonicalize()
            .ok()
            .and_then(|real| index.get(&real))
            .and_then(|candidates| cache.get(&candidates[0]));
        assert!(found.is_some(), "but the real path finds it");
    }

    /// After an aliased hit the entry must also be reachable under the spelling
    /// the scan walked: the background pass keys by that, and without it
    /// `keep_best` sees no cached entry, so a transient discovery failure would
    /// write a negative and unregister the effect next launch (#307).
    #[test]
    fn an_aliased_entry_becomes_reachable_under_the_walked_key() {
        let mut cache = cache_of(&["old-spelling.aex"]);
        apply_rekey(
            &mut cache,
            vec![("old-spelling.aex".into(), "walked.aex".into())],
        );
        let moved = cache.get("walked.aex").expect("found under the walked key");
        assert!(moved.ok);

        // And now the background merge sees it, so a failed recheck cannot demote.
        let merged = keep_best(Some(moved), failed(5, 64, build(2)), META).unwrap();
        assert!(merged.ok, "the demotion guard applies again");
    }

    /// The alias index only covers the folders this launch scanned, so a leftover
    /// key elsewhere (a disconnected drive) is never resolved at startup.
    #[test]
    fn the_alias_index_only_covers_the_scanned_roots() {
        let root = temp_root("alias-scope");
        std::fs::write(root.join("here.aex"), b"x").unwrap();
        let inside = root.join("here.aex").to_string_lossy().into_owned();
        let outside = PathBuf::from("elsewhere")
            .join("far.aex")
            .to_string_lossy()
            .into_owned();
        let cache = cache_of(&[&inside, &outside]);
        let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
        assert_eq!(index.len(), 1, "only the key under the scanned root");
        assert!(index.values().any(|keys| keys.contains(&inside)));
    }

    /// The walked spelling can be the temporary one. If the scan reached the AEX
    /// through a junction that is gone next launch, deleting the original would
    /// leave nothing that resolves, and the effect would go unregistered (#307).
    #[test]
    fn re_keying_keeps_the_original_spelling() {
        let mut cache = cache_of(&["stable.aex"]);
        apply_rekey(
            &mut cache,
            vec![("stable.aex".into(), "via-junction.aex".into())],
        );
        assert!(cache.contains_key("stable.aex"));
        assert!(cache.contains_key("via-junction.aex"));
    }

    #[test]
    fn a_live_alias_copy_does_not_keep_alias_lookup_hot() {
        let root = PathBuf::from("root");
        let alias = root.join("stable.aex").to_string_lossy().into_owned();
        let walked = root.join("walked.aex").to_string_lossy().into_owned();
        let mut cache = cache_of(&[&alias]);
        apply_rekey(&mut cache, vec![(alias.clone(), walked.clone())]);

        assert!(!alias_possible(
            &cache,
            &walked_set(&[&walked]),
            std::slice::from_ref(&root),
        ));
        cache.remove(&walked);
        assert!(
            alias_possible(&cache, &walked_set(&[&walked]), std::slice::from_ref(&root)),
            "the fallback becomes eligible again if its walked copy disappears"
        );
    }

    /// When both spellings of one file are cached and they disagree (only the
    /// walked one is refreshed by discovery), the alias lookup must land on the
    /// one that registers, and must do so every launch rather than by iteration
    /// order — otherwise the effect flickers in and out (#307).
    #[cfg(windows)]
    #[test]
    fn the_alias_index_prefers_an_entry_that_registers() {
        let root = temp_root("index-preference");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let direct = real.join("foo.aex").to_string_lossy().into_owned();
        let via_link = root
            .join("link")
            .join("foo.aex")
            .to_string_lossy()
            .into_owned();

        // Both spellings resolve to one file, so they collide in the index.
        // Whichever spelling holds the negative, the registering entry must win.
        for (negative, positive) in [(&direct, &via_link), (&via_link, &direct)] {
            let mut cache = HashMap::new();
            cache.insert(negative.clone(), failed(5, 64, build(1)));
            cache.insert(positive.clone(), discovered(5, 64, build(1)));
            let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
            assert_eq!(index.len(), 1, "both spellings resolved to one file");
            let winner = &index.values().next().unwrap()[0];
            assert!(cache[winner].ok, "the registering entry won");
        }
    }

    /// Copying an aliased entry makes "one file, two cached spellings, both
    /// registerable" the normal case, so the pick has to stay put across launches
    /// — otherwise the effect's parameters (frozen by AviUtl2 at load) change
    /// depending on hash order.
    #[cfg(windows)]
    #[test]
    fn the_alias_index_picks_the_same_spelling_every_time() {
        let root = temp_root("index-stable");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let direct = real.join("foo.aex").to_string_lossy().into_owned();
        let via_link = root
            .join("link")
            .join("foo.aex")
            .to_string_lossy()
            .into_owned();

        let mut winners = std::collections::HashSet::new();
        for _ in 0..64 {
            let mut cache = HashMap::new();
            // Both registerable and both on the current build: only the tie-break
            // decides. Different sha so the winner is identifiable.
            let mut a = discovered(5, 64, build(1));
            a.sha = "aaa".into();
            let mut b = discovered(5, 64, build(1));
            b.sha = "bbb".into();
            cache.insert(direct.clone(), a);
            cache.insert(via_link.clone(), b);
            let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
            assert_eq!(index.len(), 1);
            winners.insert(cache[&index.values().next().unwrap()[0]].sha.clone());
        }
        assert_eq!(winners.len(), 1, "one winner across runs, got {winners:?}");
    }

    /// An entry the current host produced beats a leftover from an older one.
    #[cfg(windows)]
    #[test]
    fn the_alias_index_prefers_the_current_host_build() {
        let root = temp_root("index-build");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let direct = real.join("foo.aex").to_string_lossy().into_owned();
        let via_link = root
            .join("link")
            .join("foo.aex")
            .to_string_lossy()
            .into_owned();

        for (stale_key, fresh_key) in [(&direct, &via_link), (&via_link, &direct)] {
            let mut cache = HashMap::new();
            let mut stale = discovered(5, 64, build(1));
            stale.sha = "old".into();
            let mut fresh = discovered(5, 64, build(2));
            fresh.sha = "new".into();
            cache.insert(stale_key.clone(), stale);
            cache.insert(fresh_key.clone(), fresh);
            let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(2));
            let winner = &index.values().next().unwrap()[0];
            assert_eq!(cache[winner].sha, "new", "the current build's entry won");
        }
    }

    /// Only the walked spelling is refreshed, so the copy left under another one
    /// can be the newer of the two. When the entry found directly would not
    /// register, the alias must still be consulted, or the effect is unregistered
    /// for that launch even though a usable result is cached (#307).
    #[cfg(windows)]
    #[test]
    fn a_negative_direct_hit_still_falls_back_to_a_usable_alias() {
        let root = temp_root("stale-direct");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let walked = real.join("foo.aex").to_string_lossy().into_owned();
        let other = root
            .join("link")
            .join("foo.aex")
            .to_string_lossy()
            .into_owned();

        // The spelling being walked holds an old negative; the other spelling
        // holds the result a later pass discovered.
        let mut cache = HashMap::new();
        cache.insert(walked.clone(), failed(5, 64, build(1)));
        cache.insert(other.clone(), discovered(5, 64, build(1)));

        let direct = cache.get(&walked);
        assert!(
            !classify(direct, META, build(1)).register,
            "the direct hit alone would not register"
        );
        let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
        let candidates = index
            .get(&real.join("foo.aex").canonicalize().unwrap())
            .expect("the file is in the index");
        let alias = candidates
            .iter()
            .find(|alias| classify(cache.get(*alias), META, build(1)).register)
            .expect("one of the spellings registers");
        assert_eq!(alias, &other);
    }

    // --- resolve_cached: which spelling's entry gets used --------------------

    /// Only the walked spelling is refreshed, so a copy under another one can be
    /// the newer of the two. When what is held under the walked spelling would
    /// not register, the alias must be adopted, or the effect goes unregistered
    /// for that launch and saved projects lose the objects using it (#307).
    #[cfg(windows)]
    #[test]
    fn a_negative_direct_hit_adopts_a_usable_alias() {
        let root = temp_root("resolve-adopt");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");
        let other = root
            .join("link")
            .join("foo.aex")
            .to_string_lossy()
            .into_owned();

        let mut cache = HashMap::new();
        cache.insert(
            walked.to_string_lossy().into_owned(),
            failed(5, 64, build(1)),
        );
        cache.insert(other.clone(), discovered(5, 64, build(1)));

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert!(
            entry.is_some_and(|entry| entry.ok),
            "adopted the usable entry"
        );
        assert_eq!(
            alias.as_deref(),
            Some(other.as_str()),
            "and reports the re-key"
        );
    }

    /// A spelling that already registers must not pay for the alias lookup.
    #[test]
    fn a_usable_direct_hit_never_consults_the_index() {
        let cache = cache_of(&["a.aex"]);
        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            "a.aex",
            Path::new("a.aex"),
            META,
            build(1),
            &[PathBuf::from("")],
            true,
            &mut aliases,
        );
        assert!(entry.is_some_and(|entry| entry.ok));
        assert_eq!(alias, None);
        assert!(aliases.is_none(), "the index was never built");
    }

    /// And when no other spelling can exist, the lookup is skipped outright.
    #[test]
    fn nothing_is_resolved_when_no_alias_can_exist() {
        let cache = cache_of(&["gone.aex"]);
        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            "missing.aex",
            Path::new("missing.aex"),
            META,
            build(1),
            &[PathBuf::from("")],
            false,
            &mut aliases,
        );
        assert!(entry.is_none());
        assert_eq!(alias, None);
        assert!(aliases.is_none(), "no filesystem work at all");
    }

    // --- alias_rank ----------------------------------------------------------

    /// An entry written before the `build` field existed carries the default,
    /// which is also what an unreadable current fingerprint is. Comparing them
    /// would rank the legacy entry above a freshly discovered one.
    #[test]
    fn an_unknown_build_does_not_favour_a_legacy_entry() {
        let unknown = BuildFingerprint::default();
        let legacy = discovered(5, 64, unknown);
        let fresh = discovered(5, 64, build(1));
        assert_eq!(
            alias_rank(Some(&legacy), unknown),
            alias_rank(Some(&fresh), unknown),
            "with no usable fingerprint, neither wins on build"
        );
    }

    #[test]
    fn a_current_build_entry_outranks_an_older_one() {
        let old = discovered(5, 64, build(1));
        let current = discovered(5, 64, build(2));
        assert!(alias_rank(Some(&current), build(2)) > alias_rank(Some(&old), build(2)));
    }

    /// A stale entry's sha/params may describe older bytes, so a sound entry
    /// wins even if it came from an older host.
    #[test]
    fn a_sound_entry_outranks_a_stale_one() {
        let mut stale = discovered(5, 64, build(2));
        stale.stale = true;
        let sound = discovered(5, 64, build(1));
        assert!(alias_rank(Some(&sound), build(2)) > alias_rank(Some(&stale), build(2)));
    }

    #[test]
    fn a_registerable_entry_outranks_a_negative_one() {
        let ok = discovered(5, 64, build(1));
        let negative = failed(5, 64, build(1));
        assert!(alias_rank(Some(&ok), build(1)) > alias_rank(Some(&negative), build(1)));
    }

    /// The rank cannot tell whether an entry still describes the file, so a
    /// better-ranked but outdated spelling must not shadow a usable one — that
    /// would leave the effect unregistered even though a usable result is cached
    /// (#307). Both entries here rank equally, so the tie-break orders them and
    /// the lookup has to fall through to the second.
    #[cfg(windows)]
    #[test]
    fn an_outdated_candidate_does_not_shadow_a_usable_one() {
        let root = temp_root("candidate-fallthrough");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = root.join("zzz-missing.aex"); // not cached at all

        for (outdated, usable) in [
            (real.join("foo.aex"), root.join("link").join("foo.aex")),
            (root.join("link").join("foo.aex"), real.join("foo.aex")),
        ] {
            let mut cache = HashMap::new();
            // Same rank (ok, not stale, same build); only the meta differs.
            cache.insert(
                outdated.to_string_lossy().into_owned(),
                discovered(9, 99, build(1)),
            );
            cache.insert(
                usable.to_string_lossy().into_owned(),
                discovered(5, 64, build(1)),
            );
            let mut aliases = None;
            let (entry, alias) = resolve_cached(
                &cache,
                &walked.to_string_lossy(),
                &real.join("foo.aex"),
                META,
                build(1),
                std::slice::from_ref(&root),
                true,
                &mut aliases,
            );
            assert_eq!(
                entry.map(|entry| entry.len),
                Some(64),
                "took the usable one"
            );
            assert_eq!(alias.as_deref(), Some(&*usable.to_string_lossy()));
        }
    }

    /// When no spelling registers, nothing is adopted and nothing is re-keyed.
    #[cfg(windows)]
    #[test]
    fn an_unusable_alias_is_not_adopted() {
        let root = temp_root("alias-unusable");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");

        let mut cache = HashMap::new();
        cache.insert(
            walked.to_string_lossy().into_owned(),
            failed(5, 64, build(1)),
        );
        cache.insert(
            root.join("link")
                .join("foo.aex")
                .to_string_lossy()
                .into_owned(),
            failed(5, 64, build(1)),
        );
        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert!(entry.is_some_and(|entry| !entry.ok), "kept what was there");
        assert_eq!(alias, None, "nothing worth re-keying");
    }

    // --- alias_possible ------------------------------------------------------

    fn walked_set(keys: &[&str]) -> std::collections::HashSet<String> {
        keys.iter().map(|key| (*key).to_string()).collect()
    }

    /// A cached key under a scan root that this scan did not walk is exactly the
    /// case the alias lookup exists for.
    #[test]
    fn an_unwalked_key_under_a_root_means_an_alias_may_exist() {
        let root = PathBuf::from("root");
        let cache = cache_of(&[&root.join("old-name.aex").to_string_lossy()]);
        assert!(alias_possible(
            &cache,
            &walked_set(&[&root.join("walked.aex").to_string_lossy()]),
            std::slice::from_ref(&root),
        ));
    }

    /// When every in-scope key is one the scan walked, there is no other spelling
    /// and the lookup is pure cost.
    #[test]
    fn all_keys_walked_means_no_alias_can_exist() {
        let root = PathBuf::from("root");
        let key = root.join("walked.aex").to_string_lossy().into_owned();
        let cache = cache_of(&[&key]);
        assert!(!alias_possible(
            &cache,
            &walked_set(&[&key]),
            std::slice::from_ref(&root)
        ));
    }

    /// Keys outside the scanned roots say nothing: they are never registered from
    /// and never pruned.
    #[test]
    fn keys_outside_the_roots_do_not_imply_an_alias() {
        let root = PathBuf::from("root");
        let cache = cache_of(&["elsewhere/other.aex"]);
        assert!(!alias_possible(
            &cache,
            &walked_set(&[]),
            std::slice::from_ref(&root)
        ));
    }

    /// A stale entry carries the meta just read from disk, so the no-demotion
    /// guard holds even when what is there now genuinely does not discover: the
    /// merge is a fixed point, so the entry stays registered on the older bytes'
    /// payload and is re-checked every launch instead of converging. That is
    /// deliberate — excluding stale entries here would unregister one whose
    /// re-check merely timed out, deleting objects out of saved projects (#307),
    /// and would give up the self-healing that one later success provides.
    /// Converging safely needs the failure's classification (#328). Pinned so the
    /// trade-off is not reversed by accident.
    #[test]
    fn a_stale_entry_does_not_converge_on_a_failed_recheck() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();

        // What is on disk now is the replacement, and it fails to discover.
        let replacement = Some(((9, 0), 128));
        let merged = keep_best(Some(&stale), failed(9, 128, build(1)), replacement).unwrap();

        assert!(merged.ok, "still registered, so objects survive");
        assert_eq!(merged.sha, "older-bytes", "on the older bytes' payload");
        assert!(merged.stale, "and queued again");
        assert_eq!(
            classify(Some(&merged), replacement, build(1)),
            LoadDecision {
                register: true,
                discover: true
            }
        );
        // A fixed point: re-checking again cannot move it, which is what "does
        // not converge" means here.
        assert_eq!(
            (merged.ok, merged.stale, &merged.sha),
            (stale.ok, stale.stale, &stale.sha)
        );
        assert_eq!(
            (merged.mtime, merged.len, merged.build),
            (stale.mtime, stale.len, stale.build)
        );
    }

    #[test]
    fn a_stale_entry_converges_on_a_deterministic_failure() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();
        let mut failed = failed(9, 128, build(1));
        failed.failure_classification = Some("nonzero_exit".into());

        let merged = keep_best(Some(&stale), failed, Some(((9, 0), 128))).unwrap();
        assert!(
            !merged.ok,
            "the deterministically rejected replacement is negative"
        );
        assert!(!merged.stale, "a permanent failure is no longer queued");
        assert_eq!(
            classify(Some(&merged), Some(((9, 0), 128)), build(1)),
            LoadDecision {
                register: false,
                discover: false
            }
        );
    }

    #[test]
    fn a_stale_entry_keeps_retrying_after_a_timeout_failure() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        let mut failed = failed(9, 128, build(1));
        failed.failure_classification = Some("timeout_killed".into());

        let merged = keep_best(Some(&stale), failed, Some(((9, 0), 128))).unwrap();
        assert!(merged.ok);
        assert!(merged.stale);
        assert_eq!(
            classify(Some(&merged), Some(((9, 0), 128)), build(1)),
            LoadDecision {
                register: true,
                discover: true
            }
        );
    }

    #[test]
    fn inspection_errors_preserve_the_broker_failure_classification() {
        let error = std::io::Error::other(
            r#"inspection failed: diagnostics={"classification":"crashed","exit_code":3221225477}"#,
        );
        assert_eq!(
            inspection_failure_classification(&error).as_deref(),
            Some("crashed")
        );
    }

    /// The same entry does converge as soon as a re-check succeeds.
    #[test]
    fn a_stale_entry_converges_on_a_successful_recheck() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();

        let replacement = Some(((9, 0), 128));
        let mut fresh = discovered(9, 128, build(1));
        fresh.sha = "replacement".into();
        let merged = keep_best(Some(&stale), fresh, replacement).unwrap();
        assert_eq!(merged.sha, "replacement");
        assert!(!merged.stale, "no longer queued");
    }

    /// A stale entry registers, but on a payload that may describe older bytes,
    /// so its sessions fail to open and its frames pass through unrendered. When
    /// another spelling of the same file holds a sound entry, that one must be
    /// used instead of stopping at the stale direct hit.
    #[cfg(windows)]
    #[test]
    fn a_stale_direct_hit_still_looks_for_a_sound_alias() {
        let root = temp_root("stale-vs-sound");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");
        let other = root
            .join("link")
            .join("foo.aex")
            .to_string_lossy()
            .into_owned();

        let mut stale = discovered(5, 64, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();
        let mut sound = discovered(5, 64, build(1));
        sound.sha = "current".into();

        let mut cache = HashMap::new();
        cache.insert(walked.to_string_lossy().into_owned(), stale);
        cache.insert(other.clone(), sound);

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert_eq!(entry.map(|entry| entry.sha.as_str()), Some("current"));
        assert_eq!(alias.as_deref(), Some(other.as_str()));
    }

    /// But a stale direct hit is kept when no sounder spelling exists: dropping
    /// it would unregister the effect (#307).
    #[cfg(windows)]
    #[test]
    fn a_stale_direct_hit_is_kept_when_no_alias_is_sounder() {
        let root = temp_root("stale-only");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");

        let mut stale = discovered(5, 64, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();
        let mut also_stale = discovered(5, 64, build(1));
        also_stale.stale = true;
        also_stale.sha = "other-older".into();

        let mut cache = HashMap::new();
        cache.insert(walked.to_string_lossy().into_owned(), stale);
        cache.insert(
            root.join("link")
                .join("foo.aex")
                .to_string_lossy()
                .into_owned(),
            also_stale,
        );

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert_eq!(
            entry.map(|entry| entry.sha.as_str()),
            Some("older-bytes"),
            "kept the walked spelling, still registered"
        );
        assert_eq!(alias, None, "no lateral move");
    }

    /// `alias_rank` cannot see whether an entry still describes the file, so a
    /// top-ranked direct hit can still fail to register. Choosing only strictly
    /// sounder candidates would then skip an equally ranked but usable spelling
    /// and leave the effect unregistered (#307).
    #[cfg(windows)]
    #[test]
    fn an_unusable_top_ranked_direct_hit_adopts_an_equal_ranked_alias() {
        let root = temp_root("equal-rank-adopt");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");
        let other = root
            .join("link")
            .join("foo.aex")
            .to_string_lossy()
            .into_owned();

        let mut cache = HashMap::new();
        // Same rank as the alias (ok, not stale, current build) but its meta does
        // not match the file, so it cannot be registered.
        cache.insert(
            walked.to_string_lossy().into_owned(),
            discovered(9, 99, build(1)),
        );
        cache.insert(other.clone(), discovered(5, 64, build(1)));

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert_eq!(
            entry.map(|entry| entry.len),
            Some(64),
            "took the usable one"
        );
        assert_eq!(alias.as_deref(), Some(other.as_str()));
    }

    fn parameter(slot: u32, name: &str, visible: bool) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: name.into(),
            kind: "float".into(),
            minimum: 0.0,
            maximum: 1.0,
            value: 0.0,
            choices: Vec::new(),
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 1,
            layer_path: None,
            enabled: true,
            visible,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    #[test]
    fn unique_item_names_preserve_unique_labels_and_bind_duplicates_to_slots() {
        let parameters = vec![
            parameter(1, "Intensity", true),
            parameter(2, "Intensity", true),
            parameter(3, "", true),
            parameter(4, " ", true),
            parameter(5, "Unique", true),
            parameter(6, "Hidden", false),
        ];
        let names = unique_item_names(&parameters);
        let visible: Vec<&str> = names.iter().filter_map(Option::as_deref).collect();

        assert_eq!(visible[0], "Intensity [slot 1]");
        assert_eq!(visible[1], "Intensity [slot 2]");
        assert_eq!(visible[2], "Parameter 3 [slot 3]");
        assert_eq!(visible[3], "Parameter 4 [slot 4]");
        assert_eq!(visible[4], "Unique");
        assert!(
            names[5].is_none(),
            "invisible parameters do not consume item names"
        );
        assert_eq!(visible.len(), visible.iter().collect::<HashSet<_>>().len());
    }

    #[test]
    fn generated_slot_name_collision_gets_a_second_stable_suffix() {
        let parameters = vec![
            parameter(1, "Intensity", true),
            parameter(2, "Intensity", true),
            parameter(3, "Intensity [slot 1]", true),
        ];
        let names = unique_item_names(&parameters);

        assert_eq!(names[0].as_deref(), Some("Intensity [slot 1]"));
        assert_eq!(names[1].as_deref(), Some("Intensity [slot 2]"));
        assert_eq!(names[2].as_deref(), Some("Intensity [slot 1] [2]"));
        assert_eq!(
            names
                .iter()
                .filter_map(Option::as_ref)
                .collect::<HashSet<_>>()
                .len(),
            3
        );
    }

    // --- cluster sessions (issue #405) ---

    fn artifact(name: &str, sha_byte: u8) -> ApprovedImageArtifact {
        ApprovedImageArtifact {
            path: PathBuf::from(format!(r"C:\plugins\{name}")),
            expected_sha256: [sha_byte; 32],
            expected_size: 64,
        }
    }

    fn temp_source() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aexcompat-mf-resources-{}-{nonce:032x}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn extra_inputs_add_bib_only_when_the_closure_lacks_it() {
        let root = temp_source();
        let plugin = root.join("effect.aex");
        std::fs::write(&plugin, b"effect").unwrap();
        std::fs::write(root.join("BIB.dll"), b"bib").unwrap();

        // No BIB in the closure: the host facility DLL is sealed as an extra.
        let extra = gather_extra_sealed_inputs(&plugin, &[], std::slice::from_ref(&root)).unwrap();
        assert_eq!(extra.dependencies.len(), 1);
        assert_eq!(
            extra.dependencies[0]
                .path
                .file_name()
                .and_then(|name| name.to_str()),
            Some("BIB.dll")
        );
        assert_eq!(extra.dependencies[0].expected_size, 3);

        // BIB already in the closure: nothing is added (a duplicate basename
        // would fail the sealed tree closed).
        let with_bib = vec![artifact("bib.dll", 7)];
        let extra = gather_extra_sealed_inputs(&plugin, &with_bib, &[root.clone()]).unwrap();
        assert!(extra.dependencies.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extra_inputs_gather_film_stocks_and_reject_links_and_caps() {
        let root = temp_source();
        let plugin = root.join("effect.aex");
        std::fs::write(&plugin, b"effect").unwrap();
        let stocks = root.join("Film Stocks");
        std::fs::create_dir(&stocks).unwrap();
        std::fs::write(stocks.join("100T.grain"), b"grain-a").unwrap();
        std::fs::write(stocks.join("500T.grain"), b"grain-b").unwrap();

        let extra = gather_extra_sealed_inputs(&plugin, &[], &[]).unwrap();
        assert_eq!(extra.resources.len(), 2);
        assert_eq!(extra.resources[0].relative_path, "Film Stocks/100T.grain");
        assert_eq!(extra.resources[1].relative_path, "Film Stocks/500T.grain");
        assert_eq!(extra.resources[0].expected_size, 7);

        // A non-plain entry (here a nested directory) fails closed.
        std::fs::create_dir(stocks.join("nested")).unwrap();
        assert!(gather_extra_sealed_inputs(&plugin, &[], &[]).is_err());
        std::fs::remove_dir(stocks.join("nested")).unwrap();

        // Beyond the count cap the gather fails closed instead of staging a
        // partial set the plug-in would silently misread.
        for index in 0..=MAX_SEALED_RESOURCES {
            std::fs::write(stocks.join(format!("filler-{index:04}.grain")), b"x").unwrap();
        }
        assert!(gather_extra_sealed_inputs(&plugin, &[], &[]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn closure_identity_is_order_independent_and_content_sensitive() {
        let first = vec![artifact("a.dll", 1), artifact("B.dll", 2)];
        let mut reversed = first.clone();
        reversed.reverse();
        assert_eq!(closure_identity_of(&first), closure_identity_of(&reversed));
        // Basenames fold the way the loader's collision rules fold.
        let folded = vec![artifact("A.dll", 1), artifact("b.dll", 2)];
        assert_eq!(closure_identity_of(&first), closure_identity_of(&folded));
        // Same names, different bytes: a different closure.
        let changed = vec![artifact("a.dll", 1), artifact("B.dll", 3)];
        assert_ne!(closure_identity_of(&first), closure_identity_of(&changed));
        let missing = vec![artifact("a.dll", 1)];
        assert_ne!(closure_identity_of(&first), closure_identity_of(&missing));
    }

    fn planned(identity: Option<&str>, dependency_count: usize) -> PlannedMember {
        PlannedMember {
            identity: identity.map(str::to_owned),
            dependency_count,
        }
    }

    #[test]
    fn plan_tasks_clusters_only_shareable_identities() {
        let members = vec![
            planned(Some("cluster"), 2),
            planned(Some("cluster"), 2),
            planned(Some("single"), 2),
            planned(None, 0),
            planned(Some("cluster"), 2),
        ];
        let tasks = plan_tasks(&members);
        // One cluster over members 0/1/4 (first-seen), then singles in scan
        // order for the singleton identity and the failed resolution.
        assert_eq!(tasks.len(), 3);
        match &tasks[0] {
            DiscoveryTask::Cluster(members) => assert_eq!(members, &[0, 1, 4]),
            _ => panic!("first task must be the cluster"),
        }
        for (task, expected) in tasks[1..].iter().zip([2usize, 3usize]) {
            match task {
                DiscoveryTask::Single(index) => assert_eq!(*index, expected),
                _ => panic!("expected a singleton task"),
            }
        }
    }

    #[test]
    fn plan_tasks_keeps_oversized_clusters_on_the_per_plugin_path() {
        let members: Vec<PlannedMember> = (0..=MAX_CLUSTER_PLUGINS)
            .map(|_| planned(Some("huge"), 2))
            .collect();
        let tasks = plan_tasks(&members);
        assert!(
            tasks
                .iter()
                .all(|task| matches!(task, DiscoveryTask::Single(_)))
        );
        assert_eq!(tasks.len(), MAX_CLUSTER_PLUGINS + 1);
    }

    #[test]
    fn plan_tasks_routes_oversized_singleton_closures_to_a_one_member_cluster() {
        // The threshold: deps + the measured system tail past the one-shot
        // 512-module audit cap. 446 deps still fits (446 + 66 = 512, not
        // over); 447 does not (issue #362/#478).
        let members = vec![
            planned(Some("small"), 446),
            planned(Some("large"), 447),
            planned(Some("larger"), 700),
            planned(None, 700),
            planned(Some("tiny"), 0),
        ];
        let tasks = plan_tasks(&members);
        let cluster_of = |index: usize| match &tasks[index] {
            DiscoveryTask::Cluster(members) => members.clone(),
            _ => panic!("task {index} must be a cluster"),
        };
        match &tasks[0] {
            DiscoveryTask::Single(index) => assert_eq!(*index, 0),
            _ => panic!("446 deps stays on the one-shot path"),
        }
        assert_eq!(cluster_of(1), vec![1]);
        assert_eq!(cluster_of(2), vec![2]);
        // A failed closure resolution never clusters, however large the walk
        // was: there is no authenticated identity to seal a session around.
        match &tasks[3] {
            DiscoveryTask::Single(index) => assert_eq!(*index, 3),
            _ => panic!("an unresolved closure stays on the one-shot path"),
        }
        match &tasks[4] {
            DiscoveryTask::Single(index) => assert_eq!(*index, 4),
            _ => panic!("a zero-dependency singleton stays on the one-shot path"),
        }
    }

    #[test]
    fn decode_sha256_hex_accepts_only_64_hex_digits() {
        let hex = "ab".repeat(32);
        assert_eq!(decode_sha256_hex(&hex).unwrap()[0], 0xab);
        assert!(decode_sha256_hex(&"ab".repeat(31)).is_none());
        assert!(decode_sha256_hex(&format!("{}zz", "ab".repeat(31))).is_none());
    }

    #[test]
    fn inspect_report_parameters_mirror_the_one_shot_conversion() {
        let report = serde_json::json!({
            "params_setup_error": 0,
            "out_flags2": 1 << 10,
            "parameters": [
                {"index": 1, "type": 2, "name": "Gain", "default": 0.5,
                 "valid_min": 0.0, "valid_max": 100.0},
                {"index": 2, "type": 4, "name": "Enable", "default": 1.0,
                 "valid_min": 0, "valid_max": 1, "ui_flags": 0},
                {"index": 3, "type": 7, "name": "Mode", "default": 2.0,
                 "valid_min": 1, "valid_max": 3, "choices": "A|B|C"},
                {"index": 4, "type": 5, "name": "Tint",
                 "default_color": {"alpha": 255, "red": 10, "green": 20, "blue": 30}},
                {"index": 5, "type": 99, "name": "Unknown"}
            ]
        });
        let mut entry = negative_entry(Path::new("effect.aex"), build(1));
        fill_entry_from_inspect_report(&mut entry, &report);
        assert!(entry.ok, "a clean report marks the entry discovered");
        assert!(entry.smart, "out_flags2 bit 10 advertises SmartFX");
        assert_eq!(entry.params.len(), 4, "unknown parameter kinds are skipped");
        assert_eq!(entry.params[0].kind, "float");
        assert_eq!(entry.params[0].slot, 1);
        assert_eq!(entry.params[0].value, 0.5);
        assert_eq!(entry.params[0].minimum, 0.0);
        assert_eq!(entry.params[0].maximum, 100.0);
        assert_eq!(entry.params[1].kind, "integer");
        assert_eq!(entry.params[2].choices, vec!["A", "B", "C"]);
        assert_eq!(entry.params[3].color, [255, 10, 20, 30]);

        // PARAMS_SETUP rejection is a plugin-local failure, not a discovery.
        let rejected = serde_json::json!({"params_setup_error": 25, "parameters": []});
        let mut entry = negative_entry(Path::new("effect.aex"), build(1));
        fill_entry_from_inspect_report(&mut entry, &rejected);
        assert!(!entry.ok);
        assert!(entry.params.is_empty());
    }

    // --- cluster session smoke tests against the protocol fixture (issue #405) ---

    #[cfg(windows)]
    mod cluster_smoke {
        use super::*;

        /// Serializes the fixture-driven smoke tests: the fixture's behavior
        /// is selected through the inherited process environment.
        static BEHAVIOR_LOCK: Mutex<()> = Mutex::new(());

        fn build_session_fixture() -> PathBuf {
            let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../broker/Cargo.toml");
            let status = std::process::Command::new(env!("CARGO"))
                .args(["build", "--manifest-path"])
                .arg(&manifest)
                .args(["-p", "dummy-workers", "--bin", "session_protocol_worker"])
                .status()
                .expect("run cargo build for the session protocol fixture");
            assert!(status.success(), "session protocol fixture build failed");
            manifest
                .parent()
                .expect("workspace root")
                .join("target/debug/session_protocol_worker.exe")
        }

        /// A temp repository whose `target/minihost-build/aex_render_worker.exe`
        /// is the protocol fixture and whose two "plug-ins" are two copies of
        /// one real PE image, so the closure resolver sees identical import
        /// sets — one shared closure identity, exactly the cluster shape. The
        /// L2 worker is deliberately absent: the one-shot inspect cannot
        /// succeed here, so a successful entry proves the cluster session
        /// path produced it.
        fn cluster_repository() -> (PathBuf, PathBuf, PathBuf) {
            let fixture = build_session_fixture();
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "aexcompat-mf-cluster-{}-{nonce:032x}",
                std::process::id()
            ));
            // The broker's worker-freshness gate (#613) walks
            // `<repository>/minihost/src` and refuses a worker older than the
            // newest source file, or a repository where that walk finds nothing
            // (`metadata_unavailable`, fail-closed). Give the synthetic
            // repository one source file whose mtime is far in the past:
            // `std::fs::copy` preserves the fixture's own (build-time) mtime on
            // the worker, so "now" would count as newer and still trip the gate
            // (issue #646). Same anchor the broker's own session tests use.
            let source_dir = root.join("minihost/src");
            std::fs::create_dir_all(&source_dir).unwrap();
            let anchor = source_dir.join("fixture.cpp");
            std::fs::write(&anchor, b"// freshness anchor\n").unwrap();
            let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
            std::fs::File::options()
                .write(true)
                .open(&anchor)
                .unwrap()
                .set_modified(old)
                .unwrap();
            let worker_dir = root.join("target/minihost-build");
            std::fs::create_dir_all(&worker_dir).unwrap();
            std::fs::copy(&fixture, worker_dir.join("aex_render_worker.exe")).unwrap();
            let one = root.join("one.aex");
            let two = root.join("two.aex");
            std::fs::copy(&fixture, &one).unwrap();
            std::fs::copy(&fixture, &two).unwrap();
            (root, one, two)
        }

        fn dependency() -> DependencyConfig {
            DependencyConfig {
                dirs: Vec::new(),
                module_limit: None,
                byte_limit: None,
            }
        }

        #[test]
        fn cluster_discovery_sweeps_same_closure_plugins_in_one_session() {
            let _guard = BEHAVIOR_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            unsafe {
                std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
            }
            let (root, one, two) = cluster_repository();
            let results = discover_all(&root, &[one.clone(), two.clone()], &dependency(), build(1));
            assert_eq!(results.len(), 2, "every plug-in gets a result");
            let identities: Vec<&Option<String>> = results
                .iter()
                .map(|(_, entry)| &entry.closure_identity)
                .collect();
            assert_eq!(identities[0], identities[1]);
            assert!(identities[0].is_some(), "both closures resolved");
            for (path, entry) in &results {
                assert!(
                    entry.ok,
                    "cluster session discovery must succeed for {}",
                    path.display()
                );
                assert!(entry.cluster_fallback.is_none());
            }
            std::fs::remove_dir_all(&root).unwrap();
        }

        #[test]
        fn cluster_discovery_falls_back_structurally_when_the_session_dies() {
            let _guard = BEHAVIOR_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            unsafe {
                std::env::set_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR", "crash_on_inspect");
            }
            let (root, one, two) = cluster_repository();
            let results = discover_all(&root, &[one.clone(), two.clone()], &dependency(), build(1));
            unsafe {
                std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
            }
            assert_eq!(results.len(), 2, "every plug-in gets a result");
            let first = results
                .iter()
                .find(|(path, _)| path == &one)
                .map(|(_, entry)| entry)
                .expect("the first member has an entry");
            let second = results
                .iter()
                .find(|(path, _)| path == &two)
                .map(|(_, entry)| entry)
                .expect("the second member has an entry");
            // The member the session died on is a structured failure...
            assert_eq!(
                first.failure_classification.as_deref(),
                Some("cluster_session_invalidated")
            );
            let fallback = first.cluster_fallback.as_ref().expect("fallback note");
            assert_eq!(fallback.at_member, 0);
            assert_eq!(fallback.resolution, "invalidated");
            assert!(!first.ok, "a dead session is never rounded to success");
            // ...and the remaining member was re-inspected per-plugin, which
            // fails here (no L2 worker by design) but carries the note.
            let fallback = second.cluster_fallback.as_ref().expect("fallback note");
            assert_eq!(fallback.at_member, 0);
            assert_eq!(fallback.resolution, "one_shot_fallback");
            std::fs::remove_dir_all(&root).unwrap();
        }

        #[test]
        fn render_close_audit_failure_is_recorded_not_rounded_to_success() {
            let _guard = BEHAVIOR_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            unsafe {
                std::env::set_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR", "audit_undeclared_module");
            }
            let (root, one, two) = cluster_repository();
            let sha_of = |path: &Path| {
                let bytes = std::fs::read(path).unwrap();
                hex_lower(&Sha256::digest(&bytes))
            };
            SESSION_CLOSE_FAILURES
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
            let session = open_mf_session(MfSessionConfig {
                repository: root.clone(),
                plugin: one.clone(),
                dependency: dependency(),
                sha: sha_of(&one),
                smart: false,
                defaults: Vec::new(),
                identity: GeomIdentity {
                    width: 8,
                    height: 4,
                    time_step: 1,
                    total_time: 300,
                    time_scale: 30,
                },
                cluster: Some(ClusterLaunch {
                    plugins: vec![(one.clone(), sha_of(&one)), (two.clone(), sha_of(&two))],
                    swap_payloads: vec![None, None],
                }),
            })
            .expect("open cluster render session");
            let tx = session.sender().expect("fresh session sender");
            // The frame renders; the close-time cluster module audit fails
            // only afterwards (the fixture injects an undeclared module into
            // the final report's observed union).
            let reply = render_on(&tx, 0, 0, vec![7u8; 8 * 4 * 4], None);
            assert!(
                matches!(reply, FrameReply::Rendered(_)),
                "the frame renders"
            );
            // The session thread's recv loop ends only when every sender is
            // gone, so the reply clone goes first; dropping the handle then
            // drives RenderSession::close on the session thread, and the
            // close-time audit failure must be recorded, never dropped with
            // the close summary.
            drop(tx);
            drop(session);
            let failures: Vec<SessionCloseFailure> = SESSION_CLOSE_FAILURES
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            unsafe {
                std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
            }
            let failure = failures
                .iter()
                .find(|failure| failure.plugin == one)
                .expect("the close-time audit failure is recorded");
            assert!(failure.close_reason.contains("module_audit"), "{failure:?}");
            assert!(failure.clustered);
            assert!(!failure.smart);
            assert_eq!(failure.frames_ok, 1, "{failure:?}");
            assert_eq!(failure.frames_errored, 0, "{failure:?}");
            assert_eq!(failure.fallback, "reopen_fresh_session");
            assert_eq!(failure.plugin_sha256, sha_of(&one));
            assert_eq!(
                failure.worker.file_name().and_then(|name| name.to_str()),
                Some("aex_render_worker.exe")
            );
            assert_eq!(failure.worker_sha256.as_deref().map(str::len), Some(64));
            assert!(
                failure
                    .module_audit
                    .as_deref()
                    .is_some_and(|detail| detail.contains("module audit")),
                "{failure:?}"
            );
            std::fs::remove_dir_all(&root).unwrap();
        }
    }
}
