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
        let limits = collect_aex(&[dir], &[]).limits;
        assert!(limits.authoritative(), "{limits:?}");
    }

    /// Nests `levels` folders under `root` and returns the deepest one.
    ///
    /// One character per level, so a cap-relative fixture stays clear of Windows
    /// `MAX_PATH` (260): a `%TEMP%` root of ~60 characters leaves room for
    /// roughly 95 levels before `create_dir_all` starts failing for a reason that
    /// has nothing to do with the test.
    fn nest(root: &Path, levels: usize) -> PathBuf {
        let mut dir = root.to_path_buf();
        for _ in 0..levels {
            dir = dir.join("d");
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The cap has to clear a real install. After Effects 2026 nests Qt resources
    /// at `Plug-ins\Effects\mochaAE\Resources\mochaui\qml\QtQuick\Dialogs\
    /// quickimpl\qml\+Fusion` — depth 10. The old cap of 8 stopped there, so
    /// every launch reported a non-authoritative scan and the prune never ran
    /// (issue #660). Nothing below was an `.aex`, so nothing was lost; what broke
    /// was the completeness judgement.
    #[test]
    fn a_tree_as_deep_as_a_real_install_scans_authoritatively() {
        // The measured install depth is 10. Pinned as an absolute number, not as
        // `MAX_SCAN_DEPTH - n`: the point is that the cap clears a real install
        // with room to spare, which a cap-relative fixture cannot fail to satisfy.
        const REAL_INSTALL_DEPTH: usize = 10;
        const {
            assert!(
                MAX_SCAN_DEPTH >= REAL_INSTALL_DEPTH * 2,
                "the cap wants margin over a real install, not to just clear it"
            )
        };

        let root = TempRoot::new("depth-real");
        let deep = nest(root.path(), REAL_INSTALL_DEPTH + 2);
        std::fs::write(deep.join("Deep.aex"), b"MZ").unwrap();

        let scan = collect_aex(&[root.path().to_path_buf()], &[]);

        assert!(scan.limits.authoritative(), "{:?}", scan.limits);
        assert_eq!(scan.seen.len(), 1, "the .aex that deep has to be found");
    }

    /// The cap still exists — it is the backstop against a pathological tree
    /// eating the stack — and hitting it reports the depth, not a folder that
    /// could not be read. Those need different fixes from the user.
    #[test]
    fn a_tree_past_the_cap_reports_the_depth_not_a_read_failure() {
        let root = TempRoot::new("depth-cap");
        nest(root.path(), MAX_SCAN_DEPTH + 2);

        let scan = collect_aex(&[root.path().to_path_buf()], &[]);

        assert!(scan.limits.too_deep, "{:?}", scan.limits);
        assert!(
            !scan.limits.unreadable,
            "a deep tree is not an unreadable one: {:?}",
            scan.limits
        );
        assert!(!scan.limits.authoritative());
    }

    #[test]
    fn an_unreadable_folder_marks_the_scan_incomplete() {
        let missing =
            std::env::temp_dir().join(format!("aexcompat-mf-{}-missing", std::process::id()));
        let scan = collect_aex(&[missing], &[]);
        assert!(scan.plugins.is_empty());
        assert!(
            !scan.limits.authoritative(),
            "a folder that could not be read is not a complete scan"
        );
        // Which reason, not just that there is one: a mistyped `dir` reaches here,
        // and reporting it as the depth cap sends the user after a limit they
        // cannot change while withholding the path advice they need (issue #660).
        assert!(scan.limits.unreadable, "{:?}", scan.limits);
        assert!(!scan.limits.too_deep, "{:?}", scan.limits);
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
        assert!(scan.limits.authoritative());
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
            !scan.limits.authoritative(),
            "an unresolvable link is 'not looked at', not 'nothing there'"
        );
        // Which reason: a link whose target is unreachable is a path the user can
        // act on, so it must not be reported as the depth cap — that withholds
        // the path remedy and blames a limit they cannot change (issue #660).
        assert!(scan.limits.unreadable, "{:?}", scan.limits);
        assert!(!scan.limits.too_deep, "{:?}", scan.limits);

        let hidden = scanned
            .join("linked")
            .join("deep.aex")
            .to_string_lossy()
            .into_owned();
        let mut cache = cache_of(&[&hidden]);
        prune_cache(
            &mut cache,
            &scan.seen,
            &[scanned],
            scan.limits.authoritative(),
        );
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
        assert!(
            scan.limits.authoritative(),
            "reaching it twice is not an incomplete scan"
        );
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
        prune_cache(&mut cache, &scan.seen, &[root], scan.limits.authoritative());
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

    // --- filter name collisions (issue #661) --------------------------------

    /// The shipped case: After Effects installs `Threshold.aex` twice, in
    /// `Effects` and in `Effects\CycoreFXHD`. Registering both as `Threshold`
    /// makes AviUtl2 refuse the second with a modal dialog that blocks startup,
    /// and loses that effect. Only the colliding pair is qualified — every other
    /// filter has to keep the name the user already knows.
    #[test]
    fn a_colliding_stem_is_qualified_by_its_folder_and_others_are_left_alone() {
        let plugins = vec![
            PathBuf::from(r"C:\AE\Plug-ins\Effects\CycoreFXHD\Threshold.aex"),
            PathBuf::from(r"C:\AE\Plug-ins\Effects\Levels.aex"),
            PathBuf::from(r"C:\AE\Plug-ins\Effects\Threshold.aex"),
        ];

        assert_eq!(
            unique_filter_names(&plugins, &[]),
            vec![
                "Threshold (CycoreFXHD)".to_owned(),
                "Levels".to_owned(),
                "Threshold (Effects)".to_owned(),
            ]
        );
    }

    /// The plug-in that this launch could not see still holds its claim on the
    /// name. Without that, a folder that momentarily could not be read renames a
    /// filter that *did* register — and AviUtl2 drops saved objects whose filter
    /// name no longer exists, which is the #307 data loss all over again.
    #[test]
    fn a_plugin_the_scan_missed_still_counts_toward_collisions() {
        let registered = vec![PathBuf::from(r"C:\AE\Plug-ins\Effects\Threshold.aex")];
        let unseen = vec![PathBuf::from(
            r"C:\AE\Plug-ins\Effects\CycoreFXHD\Threshold.aex",
        )];

        assert_eq!(
            unique_filter_names(&registered, &unseen),
            vec!["Threshold (Effects)".to_owned()],
            "the surviving filter keeps the qualified name it registered under"
        );
    }

    /// ...but a plug-in listed in both sets is one plug-in, not a collision with
    /// itself. Counting it twice would qualify a name that is actually unique.
    #[test]
    fn a_plugin_in_both_sets_is_not_a_collision_with_itself() {
        let registered = vec![PathBuf::from(r"C:\AE\Plug-ins\Effects\Threshold.aex")];
        // Same file, as the cache spells it (Windows paths are case-insensitive).
        let known = vec![PathBuf::from(r"c:\ae\plug-ins\effects\threshold.aex")];

        assert_eq!(
            unique_filter_names(&registered, &known),
            vec!["Threshold".to_owned()]
        );
    }

    /// Stems are matched case-insensitively, because Windows filenames are:
    /// `Threshold.aex` and `threshold.aex` in *different* folders are two files
    /// competing for one name, and calling them distinct hands the host a pair it
    /// may still reject — the failure this exists to prevent.
    #[test]
    fn stems_are_matched_case_insensitively() {
        let plugins = vec![
            PathBuf::from(r"C:\AE\Plug-ins\A\Threshold.aex"),
            PathBuf::from(r"C:\AE\Plug-ins\B\threshold.aex"),
        ];
        let names = unique_filter_names(&plugins, &[]);

        assert_eq!(
            names,
            vec!["Threshold (A)".to_owned(), "threshold (B)".to_owned()]
        );
    }

    /// The *generated* names must be compared case-insensitively too, not just
    /// the stems. Two plug-ins in different trees whose folders differ only in
    /// case yield `Threshold (Fx)` and `Threshold (fx)` — distinct as bytes, the
    /// same name to the host, so the modal dialog comes back unless the final
    /// uniqueness pass folds case as well.
    #[test]
    fn qualified_names_differing_only_in_case_are_separated() {
        let plugins = vec![
            PathBuf::from(r"C:\X\Fx\Threshold.aex"),
            PathBuf::from(r"C:\Y\fx\Threshold.aex"),
        ];
        let names = unique_filter_names(&plugins, &[]);

        assert_eq!(
            names,
            vec!["Threshold (Fx)".to_owned(), "Threshold (fx) [2]".to_owned()],
            "the second has to be pushed off the name the first took"
        );
    }

    /// Same stem *and* same folder name, in different trees: qualifying by folder
    /// is not enough, so the numeric pass has to finish the job. Without it the
    /// host is handed a duplicate again and startup blocks exactly as before.
    #[test]
    fn a_collision_the_folder_cannot_separate_falls_back_to_a_number() {
        let plugins = vec![
            PathBuf::from(r"C:\AE\2025\Effects\Threshold.aex"),
            PathBuf::from(r"C:\AE\2026\Effects\Threshold.aex"),
        ];
        let names = unique_filter_names(&plugins, &[]);

        assert_eq!(
            names,
            vec![
                "Threshold (Effects)".to_owned(),
                "Threshold (Effects) [2]".to_owned(),
            ]
        );
    }

    /// Whatever the input, the host must never see one name twice — that is the
    /// entire contract.
    #[test]
    fn no_two_filters_ever_share_a_name() {
        let plugins = vec![
            PathBuf::from(r"C:\AE\Effects\Threshold.aex"),
            PathBuf::from(r"C:\AE\CycoreFXHD\Threshold.aex"),
            PathBuf::from(r"C:\AE\Other\threshold.aex"),
            // Already spelled like a generated name: must not be able to steal one.
            PathBuf::from(r"C:\AE\More\Threshold (Effects).aex"),
            PathBuf::from(r"C:\AE\Effects\Levels.aex"),
        ];
        let names = unique_filter_names(&plugins, &[]);

        let lowered: HashSet<String> = names.iter().map(|name| name.to_lowercase()).collect();
        assert_eq!(lowered.len(), plugins.len(), "{names:?}");
    }

    /// The cache `cached_naming_peers` reads: AEX path -> its entry.
    fn peer_cache(entries: &[(&str, CacheEntry)]) -> HashMap<String, CacheEntry> {
        entries
            .iter()
            .map(|(key, entry)| ((*key).to_owned(), entry.clone()))
            .collect()
    }

    /// `cached_naming_peers` returns `HashMap` iteration order, which is not
    /// stable; the caller only counts, so order is irrelevant there. Sort before
    /// comparing so a test cannot pass or fail by luck.
    fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
        paths.sort();
        paths
    }

    /// A plug-in that has never discovered successfully is not registered, but it
    /// is on disk and takes the name the moment it does discover. Filtering these
    /// out — the obvious thing to copy from `cached_fallback_plugins` — would let
    /// the registered twin claim the unqualified name and then lose it later,
    /// which is the rename this whole peer set exists to prevent.
    #[test]
    fn naming_peers_keep_plugins_that_never_discovered() {
        let cache = peer_cache(&[
            (r"C:\AE\Effects\Threshold.aex", discovered(5, 64, build(1))),
            (
                r"C:\AE\Effects\CycoreFXHD\Threshold.aex",
                failed(5, 64, build(1)),
            ),
        ]);
        let roots = vec![PathBuf::from(r"C:\AE\Effects")];

        assert_eq!(
            sorted(cached_naming_peers(&cache, &roots, false, false, &[])),
            sorted(vec![
                PathBuf::from(r"C:\AE\Effects\Threshold.aex"),
                PathBuf::from(r"C:\AE\Effects\CycoreFXHD\Threshold.aex"),
            ])
        );
    }

    /// A complete scan already listed every plug-in that can register, so the
    /// cache can only speak for files that are gone. The prune that would remove
    /// them runs only when the background pass does, so an uninstalled plug-in's
    /// key outlives it — and counting it qualifies a name whose rival no longer
    /// exists, renaming a filter that saved projects refer to.
    #[test]
    fn a_complete_scan_takes_no_peers_from_the_cache() {
        let cache = peer_cache(&[(
            // Uninstalled, not yet pruned.
            r"C:\AE\Effects\CycoreFXHD\Threshold.aex",
            discovered(5, 64, build(1)),
        )]);

        assert_eq!(
            cached_naming_peers(&cache, &[PathBuf::from(r"C:\AE")], true, false, &[]),
            Vec::<PathBuf>::new()
        );
    }

    /// A re-keyed entry is kept under both spellings on purpose, so counting both
    /// would invent a collision for a plug-in that has none — qualifying a name
    /// that was unique and dropping the saved objects that used it.
    #[test]
    fn naming_peers_drop_a_retained_alias_spelling() {
        let mut alias = discovered(5, 64, build(1));
        alias.alias_fallback = true;
        alias.alias_target = Some(r"C:\AE\Effects\Threshold.aex".to_owned());
        let cache = peer_cache(&[
            (r"C:\AE\Effects\Threshold.aex", discovered(5, 64, build(1))),
            (r"C:\AE\Legacy\Threshold.aex", alias),
        ]);
        let roots = vec![PathBuf::from(r"C:\AE")];

        assert_eq!(
            sorted(cached_naming_peers(&cache, &roots, false, false, &[])),
            sorted(vec![PathBuf::from(r"C:\AE\Effects\Threshold.aex")]),
            "one file must count once"
        );
    }

    /// ...but once the walked spelling is gone, the retained one is the only
    /// record that the plug-in exists, so it counts again.
    #[test]
    fn naming_peers_keep_an_alias_whose_target_is_gone() {
        let mut alias = discovered(5, 64, build(1));
        alias.alias_fallback = true;
        alias.alias_target = Some(r"C:\AE\Effects\Threshold.aex".to_owned());
        let cache = peer_cache(&[(r"C:\AE\Legacy\Threshold.aex", alias)]);

        assert_eq!(
            sorted(cached_naming_peers(
                &cache,
                &[PathBuf::from(r"C:\AE")],
                false,
                false,
                &[]
            )),
            sorted(vec![PathBuf::from(r"C:\AE\Legacy\Threshold.aex")])
        );
    }

    /// Ignored plug-ins are never registered, so they cannot collide with
    /// anything and must not qualify anyone else's name.
    #[test]
    fn naming_peers_drop_ignored_plugins() {
        let cache = peer_cache(&[
            (r"C:\AE\Effects\Threshold.aex", discovered(5, 64, build(1))),
            (
                r"C:\AE\Effects\CycoreFXHD\Threshold.aex",
                discovered(5, 64, build(1)),
            ),
        ]);
        let roots = vec![PathBuf::from(r"C:\AE")];

        assert_eq!(
            sorted(cached_naming_peers(
                &cache,
                &roots,
                false,
                false,
                &["Threshold".to_owned()]
            )),
            Vec::<PathBuf>::new()
        );
    }

    /// Entries outside the scanned roots belong to a folder this launch is not
    /// registering from, so they cannot collide either.
    #[test]
    fn naming_peers_drop_entries_outside_the_roots() {
        let cache = peer_cache(&[
            (r"C:\AE\Effects\Threshold.aex", discovered(5, 64, build(1))),
            (r"D:\Elsewhere\Threshold.aex", discovered(5, 64, build(1))),
        ]);

        assert_eq!(
            sorted(cached_naming_peers(
                &cache,
                &[PathBuf::from(r"C:\AE")],
                false,
                false,
                &[]
            )),
            sorted(vec![PathBuf::from(r"C:\AE\Effects\Threshold.aex")])
        );
    }

    /// ...unless the roots themselves could not be resolved. Then they do not
    /// describe where the plug-ins are, and filtering on them would drop exactly
    /// the peers this set exists to keep — the same reasoning
    /// `cached_fallback_plugins` applies.
    #[test]
    fn unresolved_roots_do_not_filter_the_naming_peers() {
        let cache = peer_cache(&[(r"D:\Elsewhere\Threshold.aex", discovered(5, 64, build(1)))]);

        assert_eq!(
            sorted(cached_naming_peers(
                &cache,
                &[PathBuf::from(r"C:\AE")],
                false,
                true,
                &[]
            )),
            sorted(vec![PathBuf::from(r"D:\Elsewhere\Threshold.aex")])
        );
    }

    /// A rename is user-visible and changes how saved projects resolve the filter
    /// (issue #662), so it has to be traceable — and the name alone cannot say
    /// which file took it once a numeric fallback is involved.
    #[test]
    fn the_rename_report_pairs_each_name_with_its_plugin() {
        let plugins = vec![
            PathBuf::from(r"C:\AE\Effects\Threshold.aex"),
            PathBuf::from(r"C:\AE\Effects\Levels.aex"),
        ];
        let names = vec!["Threshold (Effects)".to_owned(), "Levels".to_owned()];
        let summary = qualified_names_summary(&plugins, &names);

        assert!(summary.contains("Threshold (Effects)"), "{summary}");
        assert!(
            summary.contains(r"C:\AE\Effects\Threshold.aex"),
            "the path is what identifies the file: {summary}"
        );
        assert!(
            !summary.contains("Levels"),
            "a filter that kept its name is not a rename: {summary}"
        );
    }

    /// Nothing renamed is nothing to say. An empty line would still reach the log
    /// as a bare `[AEXCompat] ` prefix.
    #[test]
    fn the_rename_report_is_silent_when_nothing_was_renamed() {
        let plugins = vec![PathBuf::from(r"C:\AE\Effects\Levels.aex")];

        assert!(
            qualified_names_summary(&plugins, &["Levels".to_owned()]).is_empty(),
            "no rename, no line"
        );
    }

    /// The list is bounded, and says so rather than silently truncating — a
    /// truncated list reads as the complete set.
    #[test]
    fn the_rename_report_admits_what_it_left_out() {
        let plugins: Vec<PathBuf> = (0..12)
            .map(|index| PathBuf::from(format!(r"C:\AE\F{index}\Threshold.aex")))
            .collect();
        let names: Vec<String> = (0..12)
            .map(|index| format!("Threshold (F{index})"))
            .collect();
        let summary = qualified_names_summary(&plugins, &names);

        assert!(summary.starts_with("12 filter name(s)"), "{summary}");
        assert!(summary.contains("and 4 more"), "{summary}");
    }

    /// A path with no file name at all falls back to a placeholder rather than an
    /// empty name, and two of them still get separated. Reaching the fallback
    /// needs a path `file_stem` cannot answer for — a bare root, not a dotfile
    /// (`.aex` has `.aex` as its stem).
    #[test]
    fn a_path_without_a_stem_falls_back_to_a_placeholder() {
        let plugins = vec![PathBuf::from(r"C:\"), PathBuf::from(r"D:\")];

        assert!(
            plugins.iter().all(|path| path.file_stem().is_none()),
            "the fixture has to actually reach the fallback"
        );
        assert_eq!(
            unique_filter_names(&plugins, &[]),
            vec!["AEX".to_owned(), "AEX [2]".to_owned()],
            "no parent folder to qualify with, so only the numeric pass separates them"
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
            // The broker's worker-freshness check (#613, a recorded warning
            // rather than a gate since #729) walks `<repository>/minihost/src`
            // and flags a worker older than the newest source file. Give the
            // synthetic repository one source file whose mtime is far in the
            // past so the fixture stays warning-free: `std::fs::copy` preserves
            // the fixture's own (build-time) mtime on the worker, so "now"
            // would count as newer (issue #646). Same anchor the broker's own
            // session tests use.
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
        fn removed_staged_escape_hatch_cannot_change_discovery_route() {
            let _guard = BEHAVIOR_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            unsafe {
                std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
                // Issue #816: a stale deployment setting must not resurrect
                // closure walking or sealed staging.
                std::env::set_var("AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY", "1");
            }
            let (root, one, two) = cluster_repository();
            let results = discover_all(&root, &[one.clone(), two.clone()], &dependency(), build(1));
            unsafe {
                std::env::remove_var("AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY");
            }
            assert_eq!(results.len(), 2, "every plug-in gets a result");
            let identities: Vec<&Option<String>> = results
                .iter()
                .map(|(_, entry)| &entry.closure_identity)
                .collect();
            assert_eq!(identities[0], identities[1]);
            assert!(
                identities[0]
                    .as_deref()
                    .is_some_and(|identity| identity.starts_with("in-place:")),
                "the retired variable cannot select a staged identity"
            );
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

        /// The in-place default (issue #751): the same two plug-ins cluster
        /// by their shared search-root set, sweep in one in-place session,
        /// and record the search-root identity the render cluster pool
        /// groups on.
        #[test]
        fn in_place_cluster_discovery_sweeps_same_root_plugins_in_one_session() {
            let _guard = BEHAVIOR_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            unsafe {
                std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
                std::env::remove_var("AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY");
            }
            let (root, one, two) = cluster_repository();
            let results = discover_all(&root, &[one.clone(), two.clone()], &dependency(), build(1));
            assert_eq!(results.len(), 2, "every plug-in gets a result");
            for (path, entry) in &results {
                assert!(
                    entry.ok,
                    "in-place cluster discovery must succeed for {}",
                    path.display()
                );
                assert!(entry.cluster_fallback.is_none());
                assert!(
                    entry
                        .closure_identity
                        .as_deref()
                        .is_some_and(|identity| identity.starts_with("in-place:")),
                    "in-place discovery records the search-root identity"
                );
                assert!(
                    entry.closure.sealed.is_empty(),
                    "in-place discovery walks no closure"
                );
            }
            std::fs::remove_dir_all(&root).unwrap();
        }

        #[test]
        fn cluster_discovery_falls_back_structurally_when_the_session_dies() {
            let _guard = BEHAVIOR_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            // The in-place member the session died on is a structured
            // failure; the rest re-inspect per-plugin with the note.
            unsafe {
                std::env::set_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR", "crash_on_inspect");
                std::env::remove_var("AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY");
            }
                let (root, one, two) = cluster_repository();
                let results =
                    discover_all(&root, &[one.clone(), two.clone()], &dependency(), build(1));
                unsafe {
                    std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
                    std::env::remove_var("AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY");
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
                    Some("cluster_session_invalidated"),
                    "in-place session failure"
                );
                let fallback = first.cluster_fallback.as_ref().expect("fallback note");
                assert_eq!(fallback.at_member, 0);
                assert_eq!(fallback.resolution, "invalidated");
                assert!(!first.ok, "a dead session is never rounded to success");
                // ...and the remaining member was re-inspected per-plugin,
                // which fails here (no L2 worker by design) but carries the
                // note.
                let fallback = second.cluster_fallback.as_ref().expect("fallback note");
                assert_eq!(fallback.at_member, 0);
                assert_eq!(fallback.resolution, "one_shot_fallback");
            std::fs::remove_dir_all(&root).unwrap();
        }

        #[test]
        fn in_place_cluster_sharding_keeps_membership_and_degrades_singletons() {
            let cluster = |indices: &[usize]| DiscoveryTask::Cluster(indices.to_vec());
            let members: Vec<usize> = (0..7).collect();
            // parallelism 3 over 7 members: 3 chunks (3/3/1), the remainder
            // degrading to a per-plugin task; membership is preserved.
            let sharded = shard_in_place_clusters(vec![cluster(&members)], 3);
            let mut seen = Vec::new();
            let mut cluster_count = 0;
            for task in &sharded {
                match task {
                    DiscoveryTask::Cluster(chunk) => {
                        assert!(chunk.len() >= 2, "no one-member cluster chunks");
                        cluster_count += 1;
                        seen.extend(chunk.iter().copied());
                    }
                    DiscoveryTask::Single(index) => seen.push(*index),
                }
            }
            assert_eq!(seen, members, "sharding preserves order and membership");
            assert_eq!(cluster_count, 2);
            // Two-member clusters and singles pass through untouched.
            let untouched = shard_in_place_clusters(vec![cluster(&[0, 1])], 8);
            assert!(matches!(&untouched[0], DiscoveryTask::Cluster(chunk) if chunk.len() == 2));
            // len == 3 with ample parallelism: chunk_count is bounded by
            // len/2, so the cluster stays whole (never a one-member chunk).
            let three = shard_in_place_clusters(vec![cluster(&[0, 1, 2])], 8);
            assert_eq!(three.len(), 1);
            assert!(matches!(&three[0], DiscoveryTask::Cluster(chunk) if chunk.len() == 3));
        }

        #[test]
        fn render_close_audit_warning_closes_clean_and_records_no_failure() {
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
                layers: Vec::new(),
                cluster: Some(ClusterLaunch {
                    plugins: vec![(one.clone(), sha_of(&one)), (two.clone(), sha_of(&two))],
                    swap_payloads: vec![None, None],
                }),
            })
            .expect("open cluster render session");
            let tx = session.sender().expect("fresh session sender");
            // The frame renders; the close-time cluster module audit sees an
            // undeclared module only afterwards (the fixture injects it into
            // the final report's observed union).
            let reply = render_on(&tx, 0, 0, vec![7u8; 8 * 4 * 4], None, None);
            assert!(
                matches!(reply, FrameReply::Rendered(_)),
                "the frame renders"
            );
            // The session thread's recv loop ends only when every sender is
            // gone, so the reply clone goes first; dropping the handle then
            // drives RenderSession::close on the session thread. Since issue
            // #730 the audit outcome is a recorded warning on the close
            // report, not an invalidation: the close stays clean and no
            // close failure is recorded for the delivered frame.
            drop(tx);
            drop(session);
            let failures: Vec<SessionCloseFailure> = SESSION_CLOSE_FAILURES
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            unsafe {
                std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
            }
            assert!(
                !failures.iter().any(|failure| failure.plugin == one),
                "an audit-only close records no failure (issue #730): {failures:?}"
            );
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    // --- virtual buffer unpack (issue #645) ----------------------------------

    /// Little-endian bytes of one RGBA16F pixel.
    fn px16f(r: f32, g: f32, b: f32, a: f32) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (i, v) in [r, g, b, a].into_iter().enumerate() {
            let h = half::f16::from_f32(v).to_le_bytes();
            out[i * 2] = h[0];
            out[i * 2 + 1] = h[1];
        }
        out
    }

    #[test]
    fn unpack_converts_rgba_order_with_row_padding() {
        // 2x2, rows padded to 32 bytes (2 * 8 = 16 data + 16 pad), like the
        // D3D11 row pitch is.
        let row_pitch = 32;
        let mut bytes = vec![0u8; row_pitch * 2];
        bytes[0..8].copy_from_slice(&px16f(1.0, 0.0, 0.0, 1.0)); // red
        bytes[8..16].copy_from_slice(&px16f(0.0, 1.0, 0.0, 1.0)); // green
        bytes[row_pitch..row_pitch + 8].copy_from_slice(&px16f(0.0, 0.0, 1.0, 1.0)); // blue
        bytes[row_pitch + 8..row_pitch + 16].copy_from_slice(&px16f(0.5, 0.5, 0.5, 0.0)); // gray

        let rgba = unpack_rgba16f_to_rgba8(&bytes, 2, 2, row_pitch).unwrap();
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[0, 255, 0, 255]);
        assert_eq!(&rgba[8..12], &[0, 0, 255, 255]);
        // 0.5 in half is exactly representable; 0.5 * 255 + 0.5 rounds to 128.
        assert_eq!(&rgba[12..16], &[128, 128, 128, 0]);
    }

    /// Out-of-range and non-finite values clamp instead of wrapping: an HDR
    /// virtual buffer must not alias into wrong displacement values.
    #[test]
    fn unpack_clamps_hdr_and_nan() {
        let row_pitch = 8;
        let mut bytes = vec![0u8; row_pitch];
        bytes[0..8].copy_from_slice(&px16f(2.0, -1.0, f32::NAN, 1.0));
        let rgba = unpack_rgba16f_to_rgba8(&bytes, 1, 1, row_pitch).unwrap();
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    }

    /// A buffer shorter than the claimed geometry is refused, not misread.
    #[test]
    fn unpack_refuses_short_input() {
        assert!(unpack_rgba16f_to_rgba8(&[0u8; 15], 2, 1, 16).is_none());
        assert!(
            unpack_rgba16f_to_rgba8(&[0u8; 16], 2, 1, 8).is_none(),
            "pitch below width*8"
        );
    }

    /// The virtual-buffer wiring reads layer slots from the RAW discovery
    /// parameters. Extracting from the registered config defaults instead is
    /// the bug this pins: `build_item` maps only float/integer/color, so a
    /// layer parameter never reaches `defaults` and the wiring would be dead.
    #[test]
    fn layer_slots_come_from_raw_parameters_not_config_defaults() {
        let float = InteractiveParameter {
            slot: 1,
            name: "Amount".into(),
            kind: "float".into(),
            minimum: 0.0,
            maximum: 1.0,
            value: 0.5,
            choices: Vec::new(),
            color: [0, 0, 0, 0],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        };
        let layer = InteractiveParameter {
            slot: 2,
            name: "Displacement Map".into(),
            kind: "layer".into(),
            ..float.clone()
        };
        let raw = vec![float.clone(), layer];

        assert_eq!(layer_slots_of(&raw), vec![2], "the layer slot is found");

        // And the same parameters run through build_item (what `defaults` is
        // made of) lose the layer, proving `defaults` is the wrong source.
        let survivors: Vec<InteractiveParameter> = raw
            .iter()
            .filter_map(|parameter| build_item(parameter, "x").map(|(_, _, sent)| sent))
            .collect();
        assert!(survivors.iter().all(|parameter| parameter.kind != "layer"));
        assert!(layer_slots_of(&survivors).is_empty());
    }

    // --- worker root resolution (issue #650) ---------------------------------

    /// Lays out `<root>/target/minihost-build/aex_l2_worker.exe`.
    fn place_worker(root: &Path) {
        let dir = root.join("target/minihost-build");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("aex_l2_worker.exe"), b"MZ").unwrap();
    }

    /// A temp directory removed when the guard drops, so the suite does not
    /// leave `%TEMP%` littered.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("aexcompat-mf-root-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A deployed plugin finds its workers next to itself, with no setting at
    /// all. This is what keeps a deployment from depending on a developer's
    /// checkout, which can be deleted or moved out from under it (issue #650).
    #[test]
    fn worker_root_falls_back_to_the_plugin_directory() {
        let plugin = TempRoot::new("beside");
        place_worker(plugin.path());

        assert_eq!(
            resolve_worker_root(None, None, Some(plugin.path().join("x.aux2"))),
            Some((plugin.path().to_path_buf(), WorkerRootSource::BesidePlugin))
        );
    }

    /// The same, one level down, so a deployment can keep the workers out of the
    /// host's plugin folder proper.
    #[test]
    fn worker_root_falls_back_to_an_aexcompat_subfolder() {
        let plugin = TempRoot::new("subfolder");
        let bundle = plugin.path().join("aexcompat");
        place_worker(&bundle);

        assert_eq!(
            resolve_worker_root(None, None, Some(plugin.path().join("x.aux2"))),
            Some((bundle, WorkerRootSource::BesidePlugin))
        );
    }

    /// Nothing configured and nothing beside the plugin: report that instead of
    /// inventing a root the broker cannot run from.
    #[test]
    fn worker_root_is_none_when_nothing_holds_a_worker() {
        let plugin = TempRoot::new("empty");

        assert_eq!(
            resolve_worker_root(None, None, Some(plugin.path().join("x.aux2"))),
            None
        );
        assert_eq!(resolve_worker_root(None, None, None), None);
    }

    /// A named checkout that holds a worker wins over the plugin's own copy: a
    /// developer who names a tree means that tree.
    #[test]
    fn a_named_checkout_with_a_worker_wins_over_the_plugin_directory() {
        let plugin = TempRoot::new("named-plugin");
        place_worker(plugin.path());
        let checkout = TempRoot::new("named-checkout");
        place_worker(checkout.path());

        assert_eq!(
            resolve_worker_root(
                None,
                Some(checkout.path()),
                Some(plugin.path().join("x.aux2"))
            ),
            Some((checkout.path().to_path_buf(), WorkerRootSource::Named))
        );
        assert_eq!(
            resolve_worker_root(
                Some(checkout.path().to_path_buf()),
                None,
                Some(plugin.path().join("x.aux2"))
            ),
            Some((checkout.path().to_path_buf(), WorkerRootSource::Named)),
            "the environment override behaves the same way"
        );
    }

    /// The incident this issue is about: the named checkout is gone (a deleted
    /// worktree), so keeping it would fail every discovery again. Fall through to
    /// the workers shipped beside the plugin instead.
    #[test]
    fn a_named_checkout_without_a_worker_falls_through_to_the_plugin_directory() {
        let plugin = TempRoot::new("stale-named-plugin");
        place_worker(plugin.path());
        let gone = plugin.path().join("deleted-worktree");

        assert_eq!(
            resolve_worker_root(None, Some(&gone), Some(plugin.path().join("x.aux2"))),
            Some((plugin.path().to_path_buf(), WorkerRootSource::BesidePlugin))
        );
    }

    /// ...but with nothing beside the plugin either, the named checkout is still
    /// the answer: a developer is about to build a worker there. The reported
    /// source says the worker is missing, so the log can name that as the likely
    /// cause when nothing registers (issue #655).
    #[test]
    fn a_named_checkout_survives_when_no_worker_exists_anywhere() {
        let plugin = TempRoot::new("fresh-checkout-plugin");
        let fresh = plugin.path().join("fresh-checkout");

        assert_eq!(
            resolve_worker_root(None, Some(&fresh), Some(plugin.path().join("x.aux2"))),
            Some((fresh, WorkerRootSource::NamedWithoutWorker))
        );
    }

    /// With workers in both plugin-local candidates, the DLL's own folder wins.
    /// Pinned so the order stays a decision rather than an accident.
    #[test]
    fn the_plugin_directory_outranks_its_aexcompat_subfolder() {
        let plugin = TempRoot::new("beside-order");
        place_worker(plugin.path());
        place_worker(&plugin.path().join("aexcompat"));

        assert_eq!(
            resolve_worker_root(None, None, Some(plugin.path().join("x.aux2"))),
            Some((plugin.path().to_path_buf(), WorkerRootSource::BesidePlugin))
        );
    }

    /// The environment override outranks the config file.
    #[test]
    fn the_environment_override_outranks_the_configured_repository() {
        let plugin = TempRoot::new("precedence");
        let from_env = plugin.path().join("from-env");
        let from_config = plugin.path().join("from-config");
        place_worker(&from_env);
        place_worker(&from_config);

        assert_eq!(
            resolve_worker_root(
                Some(from_env.clone()),
                Some(&from_config),
                Some(plugin.path().join("x.aux2"))
            ),
            Some((from_env, WorkerRootSource::Named))
        );
    }

    // --- load-time logging (issue #655) --------------------------------------

    /// Knowing about plug-ins, registering none, and queueing none is the shape
    /// of the two incidents behind this work (a worker root pointing at a deleted
    /// worktree, then a worker regression failing every plug-in). The summary has
    /// to name that state instead of reporting "0 of 576" as if it were routine.
    #[test]
    fn the_summary_calls_out_registering_nothing() {
        let summary = registration_summary(576, 0, 0, AUTHORITATIVE);

        assert!(summary.contains("576"), "{summary}");
        assert!(
            summary.contains("0 of 576"),
            "the count has to be unambiguous: {summary}"
        );
        assert!(
            summary.contains("worker is failing for every plug-in"),
            "the likely cause has to be named: {summary}"
        );
        assert!(registration_is_alarming(576, 0, 0));
    }

    /// A first launch registers nothing and queues everything, which is the
    /// design working (results appear next launch), not a broken host. Reporting
    /// it in the same alarming words as the incident above would train the user
    /// to ignore the one message that matters.
    #[test]
    fn a_first_launch_is_not_reported_as_a_worker_failure() {
        let summary = registration_summary(576, 0, 576, AUTHORITATIVE);

        assert!(
            !summary.contains("failing for every plug-in"),
            "everything queued is the expected first launch: {summary}"
        );
        assert!(summary.contains("576"), "{summary}");
        assert!(
            !registration_is_alarming(576, 0, 576),
            "a first launch must not warn"
        );
    }

    /// A launch on an empty folder is not the failure either: there is nothing to
    /// register and nothing to work on, so it must not read as a broken host.
    #[test]
    fn knowing_no_plugins_is_not_reported_as_a_failure() {
        let summary = registration_summary(0, 0, 0, AUTHORITATIVE);

        assert!(
            !summary.contains("failing for every plug-in"),
            "no plug-ins is not a worker failure: {summary}"
        );
        // Positive too, so an implementation that returned "" for this input —
        // logging a bare "[AEXCompat] " line — does not pass on the negative.
        assert!(summary.contains("registered 0 of 0"), "{summary}");
        assert!(!registration_is_alarming(0, 0, 0));
    }

    /// The ordinary case still reports all three counts, so a partial failure
    /// (some registered, the rest queued) is visible without a debugger.
    #[test]
    fn the_summary_reports_registered_known_and_pending() {
        let summary = registration_summary(576, 570, 6, AUTHORITATIVE);

        assert!(summary.contains("570 of 576"), "{summary}");
        // Not a bare `contains("6")`: "576" satisfies that, so an implementation
        // that dropped the pending count would pass.
        assert!(summary.contains("6 queued"), "{summary}");
        assert!(!summary.contains("failing for every plug-in"), "{summary}");
        assert!(!registration_is_alarming(576, 570, 6));
    }

    /// A scan whose folders could not be read.
    const UNREADABLE: ScanLimits = ScanLimits {
        unresolved_root: false,
        unreadable: true,
        too_deep: false,
    };

    /// A scan that stopped at the depth cap. Distinct from [`UNREADABLE`]: the
    /// user can fix a path, they cannot fix a cap from config (issue #660).
    const TOO_DEEP: ScanLimits = ScanLimits {
        unresolved_root: false,
        unreadable: false,
        too_deep: true,
    };

    /// A launch where a *default* plug-in folder would not resolve — an AE
    /// install mid-update, a drive not yet mounted.
    const UNRESOLVED_ROOT: ScanLimits = ScanLimits {
        unresolved_root: true,
        unreadable: false,
        too_deep: false,
    };

    const AUTHORITATIVE: ScanLimits = ScanLimits {
        unresolved_root: false,
        unreadable: false,
        too_deep: false,
    };

    /// Every cause has to suppress the prune. Dropping any one of them from
    /// `authoritative()` re-enables it while this launch cannot see where the
    /// plug-ins are, which drops hundreds of cache entries, unregisters their
    /// filters next launch, and deletes the objects that used them from saved
    /// projects (issue #307).
    #[test]
    fn every_scan_limit_suppresses_the_prune() {
        for limits in [UNREADABLE, TOO_DEEP, UNRESOLVED_ROOT] {
            assert!(
                !limits.authoritative(),
                "{limits:?} must not let the prune run"
            );
        }
        assert!(AUTHORITATIVE.authoritative());
    }

    /// A default folder that would not resolve is a path problem, so it gets the
    /// path remedy — the same one an unreadable folder gets, and not the silence
    /// the depth cap gets.
    #[test]
    fn an_unresolved_root_gets_the_path_remedy() {
        let summary = empty_scan_summary(&[PathBuf::from("x")], 0, UNRESOLVED_ROOT);

        assert!(summary.contains("could not be resolved"), "{summary}");
        assert!(summary.contains("path exists"), "{summary}");
        assert!(summary.contains(ENV_DIR), "{summary}");
    }

    /// With a path cause and the depth cap at once, the remedy has to stay
    /// attached to the path cause. Appended once after the list it binds to
    /// whichever cause is last, which is how a user gets told to check `dir` for
    /// a depth limit.
    #[test]
    fn the_remedy_binds_to_the_cause_it_belongs_to() {
        let both = ScanLimits {
            unresolved_root: false,
            unreadable: true,
            too_deep: true,
        };
        let described = both.describe_with_remedy().expect("not authoritative");

        let (before, after) = described
            .split_once("a folder tree was deeper")
            .expect("the depth cause is listed");
        assert!(
            before.contains("path exists"),
            "the remedy belongs to the readable-path cause: {described}"
        );
        assert!(
            !after.contains("path exists"),
            "and must not trail the depth cause: {described}"
        );
    }

    /// The counts line names the causes without the remedy: it is a status line,
    /// and the advice belongs to the message about the folders themselves.
    #[test]
    fn the_counts_line_names_causes_without_the_remedy() {
        let summary = registration_summary(500, 500, 0, UNREADABLE);

        assert!(summary.contains("could not be read"), "{summary}");
        assert!(!summary.contains("path exists"), "{summary}");
    }

    /// The one message naming the cap has to name the number, or it cannot be
    /// compared against the tree that tripped it.
    #[test]
    fn the_depth_cap_warning_names_the_limit() {
        let warning = depth_cap_warning();

        assert!(warning.contains(&MAX_SCAN_DEPTH.to_string()), "{warning}");
        assert!(!warning.contains("config.toml"), "{warning}");
    }

    /// An untrustworthy scan pads the known set with cached plug-ins this launch
    /// never saw (#321), so the counts mean something different and the line has
    /// to say so — otherwise "registered 500 of 500" hides that 488 of them were
    /// never found on disk.
    #[test]
    fn an_incomplete_scan_is_named_in_the_summary() {
        assert!(
            registration_summary(500, 500, 0, UNREADABLE).contains("could not be read"),
            "an incomplete scan has to be admitted"
        );
        assert!(
            !registration_summary(500, 500, 0, AUTHORITATIVE).contains("could not be read"),
            "a complete scan must not claim otherwise"
        );
    }

    /// The depth cap is not a folder that could not be read. Reporting it as one
    /// sent the user to check paths and permissions for a limit they cannot reach
    /// from config, and it happened on every launch of a real AE install
    /// (issue #660).
    #[test]
    fn the_depth_cap_is_not_reported_as_an_unreadable_folder() {
        let summary = registration_summary(500, 500, 0, TOO_DEEP);

        assert!(
            summary.contains("deeper than the scan limit"),
            "the actual cause has to be named: {summary}"
        );
        assert!(
            !summary.contains("could not be read"),
            "and not the wrong one: {summary}"
        );
    }

    /// All three can hold at once and each needs a different fix, so none may
    /// hide the others.
    #[test]
    fn every_reason_a_scan_is_untrustworthy_is_listed() {
        let both = ScanLimits {
            unresolved_root: true,
            unreadable: true,
            too_deep: true,
        };
        let described = both.describe().expect("not authoritative");

        assert!(described.contains("could not be resolved"), "{described}");
        assert!(described.contains("could not be read"), "{described}");
        assert!(described.contains("deeper than"), "{described}");
        assert!(
            AUTHORITATIVE.describe().is_none(),
            "an authoritative scan has nothing to explain"
        );
        assert!(AUTHORITATIVE.authoritative());
        for limits in [UNREADABLE, TOO_DEEP] {
            assert!(!limits.authoritative(), "{limits:?}");
        }
    }

    /// Finding no .aex at all is a third way to get an empty filter list, and it
    /// used to be the quietest: `RegisterPlugin` returned before any other
    /// reporting.
    ///
    /// The line names the folders instead of guessing why they held nothing. A
    /// mistyped folder and one that exists but cannot be read both arrive here
    /// the same way — `collect_aex` only ever sees a failed `read_dir` — so a
    /// message that branched on it would tell a user with a typo to wait for a
    /// transient problem to clear.
    #[test]
    fn an_empty_scan_names_the_folders_it_searched() {
        let dirs = vec![PathBuf::from(r"C:\ProgramData\aviutl2\Plug-ins")];

        for limits in [AUTHORITATIVE, UNREADABLE] {
            let summary = empty_scan_summary(&dirs, 0, limits);
            assert!(
                summary.contains(r"C:\ProgramData\aviutl2\Plug-ins"),
                "the folder is the payload when diagnosing this: {summary}"
            );
        }
        let unreadable = empty_scan_summary(&dirs, 0, UNREADABLE);
        assert!(
            unreadable.contains("path exists"),
            "an unreadable folder has to point at the path, not at a wait: {unreadable}"
        );
        assert!(
            unreadable.contains(ENV_DIR),
            "the env override wins over config.toml, so it has to be named: {unreadable}"
        );
        assert!(
            empty_scan_summary(&[], 0, AUTHORITATIVE).contains("no folder"),
            "resolving no folder at all still has to say something"
        );
    }

    /// The depth cap gets no path advice: there is no path to fix. Offering some
    /// is the misdirection this issue is about.
    #[test]
    fn the_depth_cap_offers_no_path_advice() {
        let summary = empty_scan_summary(&[PathBuf::from("x")], 0, TOO_DEEP);

        assert!(summary.contains("deeper than the scan limit"), "{summary}");
        assert!(!summary.contains("path exists"), "{summary}");
        assert!(!summary.contains(ENV_DIR), "{summary}");
    }

    /// Ignoring every .aex found is a different cause with the same symptom, and
    /// blaming the folders for it would send the user looking in the wrong place.
    #[test]
    fn an_all_ignored_scan_blames_the_ignore_list() {
        let summary = empty_scan_summary(&[PathBuf::from("x")], 42, AUTHORITATIVE);

        assert!(summary.contains("ignore"), "{summary}");
        assert!(summary.contains("42"), "{summary}");
    }

    /// ...but an unreadable folder alongside the ignored ones still gets named:
    /// the ignore list explains only what was actually walked, and the folder
    /// that failed may be where the user's plug-ins really are.
    #[test]
    fn an_all_ignored_scan_still_admits_an_unreadable_folder() {
        let summary = empty_scan_summary(&[PathBuf::from("x")], 42, UNREADABLE);

        assert!(summary.contains("ignore"), "{summary}");
        assert!(
            summary.contains("could not be read"),
            "the incomplete scan must not be swallowed by the ignore branch: {summary}"
        );
    }

    /// The remediation has to name the path the search actually probes, and both
    /// ways a root can be named. Pointing the user at the plugin folder alone
    /// would have them drop the exe where nothing looks for it; pointing an
    /// env-var user at config.toml sends them to a file that is not in play.
    #[test]
    fn the_worker_root_wording_covers_every_source() {
        assert!(
            WorkerRootSource::BesidePlugin
                .describe()
                .contains("beside the plugin"),
            "{}",
            WorkerRootSource::BesidePlugin.describe()
        );
        for named in [
            WorkerRootSource::Named,
            WorkerRootSource::NamedWithoutWorker,
        ] {
            let described = named.describe();
            assert!(described.contains("config.toml"), "{named:?}: {described}");
            assert!(
                described.contains(ENV_REPOSITORY),
                "{named:?} must name the env override too: {described}"
            );
        }
        assert!(
            WorkerRootSource::NamedWithoutWorker
                .describe()
                .contains("no worker"),
            "a named root with no worker has to say so: {}",
            WorkerRootSource::NamedWithoutWorker.describe()
        );
    }

    /// The one message whose whole job is to be actionable. It has to name the
    /// path the search actually probes — a user who drops the exe straight into
    /// the plugin folder, as "beside the plugin" alone implies, gets the same
    /// silent empty list — and both ways to name a root.
    #[test]
    fn the_missing_worker_advice_names_the_probed_path_and_both_settings() {
        let advice = missing_worker_advice();

        assert!(
            advice.contains(L2_WORKER_RELATIVE_PATH.replace('/', "\\").as_str()),
            "the advice has to carry the probed subpath in Windows spelling, not \
             just the folder: {advice}"
        );
        assert!(advice.contains("config.toml"), "{advice}");
        assert!(advice.contains(ENV_REPOSITORY), "{advice}");
    }

    /// Rejecting every plug-in is the worker-failure signature (issue #651), not
    /// hundreds of plug-ins that all happen to be codecs.
    #[test]
    fn discovery_calls_out_rejecting_everything() {
        let summary = discovery_summary(0, 576, false, DiscoveryPassKind::Background);

        assert!(summary.contains("576"), "{summary}");
        assert!(
            summary.contains("worker is likely failing"),
            "the likely cause has to be named: {summary}"
        );
    }

    /// A few codec `.aex` among working effects is routine, so it must not be
    /// dressed up as a worker failure.
    #[test]
    fn some_rejections_alongside_effects_are_routine() {
        let summary = discovery_summary(570, 6, false, DiscoveryPassKind::Background);

        assert!(summary.contains("570"), "{summary}");
        assert!(summary.contains("6"), "{summary}");
        assert!(!summary.contains("worker is likely failing"), "{summary}");
    }

    /// A pass cut short by shutdown says so, so a partial count is not read as
    /// the final tally of what the machine supports.
    #[test]
    fn an_interrupted_pass_says_it_stopped_early() {
        assert!(
            discovery_summary(12, 3, true, DiscoveryPassKind::Background).contains("stopped early"),
            "an interrupted pass has to admit it"
        );
        assert!(
            !discovery_summary(12, 3, false, DiscoveryPassKind::Background).contains("stopped early"),
            "a complete pass must not claim it"
        );
        assert!(
            discovery_summary(0, 576, true, DiscoveryPassKind::Background).contains("stopped early"),
            "the all-rejected wording carries it too"
        );
    }

    /// Discovering nothing at all is not a failure to report: with an empty
    /// queue the pass has nothing to say about the worker — and nothing to
    /// restart AviUtl2 for either.
    #[test]
    fn an_empty_discovery_pass_is_not_reported_as_a_failure() {
        let summary = discovery_summary(0, 0, false, DiscoveryPassKind::Background);

        assert!(
            !summary.contains("worker is likely failing"),
            "nothing attempted is not a worker failure: {summary}"
        );
        assert!(
            !summary.contains("Restart"),
            "there is nothing to restart for: {summary}"
        );
        // Positive too: an empty string satisfies both negatives above and would
        // log a bare "[AEXCompat] " line.
        assert!(summary.contains("nothing was discovered"), "{summary}");
        assert!(!discovery_is_alarming(0, 0));
    }

    /// The level and the wording read the same condition, so an edit cannot leave
    /// an alarming sentence logged at info (or a routine one at warn).
    #[test]
    fn the_discovery_level_and_wording_agree() {
        for (effects, rejected) in [(0usize, 576usize), (570, 6), (0, 0), (12, 0)] {
            assert_eq!(
                discovery_is_alarming(effects, rejected),
                discovery_summary(effects, rejected, false, DiscoveryPassKind::Background).contains("worker is likely failing"),
                "({effects}, {rejected})"
            );
        }
    }

    /// The synchronous first-launch pass (issue #838) registers its results in
    /// the very launch that ran it, so its summary must not tell the user to
    /// restart AviUtl2 — that instruction belongs to the background pass alone.
    #[test]
    fn the_first_launch_pass_does_not_ask_for_a_restart() {
        let sync = discovery_summary(570, 6, false, DiscoveryPassKind::FirstLaunch);
        assert!(!sync.contains("Restart"), "{sync}");
        assert!(sync.contains("570"), "{sync}");
        assert!(sync.contains("first-launch discovery"), "{sync}");

        let background = discovery_summary(570, 6, false, DiscoveryPassKind::Background);
        assert!(background.contains("Restart AviUtl2"), "{background}");

        // The worker-failure signature (issue #651) reads the same either way:
        // where the pass ran does not change what all-rejected means.
        assert!(
            discovery_summary(0, 576, false, DiscoveryPassKind::FirstLaunch)
                .contains("worker is likely failing")
        );

        // A cut-short sync pass's remainder goes to THIS launch's background
        // pass, so its stopped-early tail must not promise the next launch —
        // that wording belongs to the background pass alone.
        let sync_cut = discovery_summary(12, 3, true, DiscoveryPassKind::FirstLaunch);
        assert!(sync_cut.contains("stopped early"), "{sync_cut}");
        assert!(sync_cut.contains("continues in the background"), "{sync_cut}");
        assert!(!sync_cut.contains("next launch"), "{sync_cut}");
        assert!(
            discovery_summary(12, 3, true, DiscoveryPassKind::Background)
                .contains("retried next launch")
        );
    }

    /// Lines captured by the fake sink below, newest last.
    static CAPTURED_LOG: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());

    unsafe fn capture(level: &'static str, message: aviutl2_sys::common::LPCWSTR) {
        let mut len = 0usize;
        // SAFETY: the caller is our own `log_line`, which passes a
        // null-terminated UTF-16 buffer alive for the duration of the call.
        unsafe {
            while *message.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(message, len));
            if let Ok(mut captured) = CAPTURED_LOG.lock() {
                captured.push((level, text));
            }
        }
    }

    unsafe extern "C" fn capture_info(
        _handle: *mut aviutl2_sys::logger2::LOG_HANDLE,
        message: aviutl2_sys::common::LPCWSTR,
    ) {
        unsafe { capture("info", message) }
    }

    unsafe extern "C" fn capture_warn(
        _handle: *mut aviutl2_sys::logger2::LOG_HANDLE,
        message: aviutl2_sys::common::LPCWSTR,
    ) {
        unsafe { capture("warn", message) }
    }

    /// The lines that matter most are written during registration, and the SDK
    /// does not promise `InitializeLogger` runs first. Holding them until the
    /// sink arrives is what keeps this issue's whole point from depending on that
    /// ordering (issue #655).
    ///
    /// One test rather than several: it installs the process-wide sink, and no
    /// other test in this binary touches `LOGGER`, `PENDING_LOG`, `log_info`,
    /// `log_warn`, or the reporting wrappers — so splitting it would make the
    /// pieces order-dependent under the test harness's threads.
    #[test]
    fn the_host_log_receives_every_line_in_order_and_at_its_level() {
        log_warn("held first");
        log_info("held second");

        let handle: &'static mut aviutl2_sys::logger2::LOG_HANDLE =
            Box::leak(Box::new(aviutl2_sys::logger2::LOG_HANDLE {
                log: capture_info,
                info: capture_info,
                warn: capture_warn,
                error: capture_warn,
                verbose: capture_info,
            }));
        set_logger(handle as *mut _);

        // Lines written once the sink exists must still arrive: an implementation
        // that only ever flushed the buffer would pass without this.
        log_info("after the sink");

        // A null handle is nothing to publish. Storing it would un-publish the
        // working sink and strand every later line in the buffer, and flushing
        // through it would call a null function pointer.
        set_logger(std::ptr::null_mut());
        log_info("after a null handle");

        let captured = CAPTURED_LOG.lock().unwrap();
        let lines: Vec<(&str, &str)> = captured
            .iter()
            .map(|(level, text)| (*level, text.as_str()))
            .collect();

        for expected in [
            "held first",
            "held second",
            "after the sink",
            "after a null handle",
        ] {
            assert_eq!(
                lines
                    .iter()
                    .filter(|(_, text)| text.contains(expected))
                    .count(),
                1,
                "{expected:?} must appear exactly once, not dropped and not \
                 double-emitted: {lines:?}"
            );
        }
        assert_eq!(
            lines.iter().map(|(level, _)| *level).collect::<Vec<&str>>(),
            vec!["warn", "info", "info", "info"],
            "each line has to reach the sink at its own level, in order: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .all(|(_, text)| text.starts_with("[AEXCompat] ")),
            "the plugin has to be identifiable in a shared log: {lines:?}"
        );
    }
    /// The F16C row must produce exactly what the reference row does, or the
    /// map silently changes with the CPU it runs on (issue #674).
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn the_f16c_row_matches_the_scalar_row_byte_for_byte() {
        if !f16c_row_available() {
            return;
        }
        // Values spanning the interesting cases: below 0, 0, the rounding
        // boundaries, 1, above 1, infinities and NaN, plus a spread in between.
        let mut source = Vec::new();
        let mut push = |value: f32| {
            source.extend_from_slice(&half::f16::from_f32(value).to_le_bytes());
        };
        for value in [
            -1.0f32,
            -0.0,
            0.0,
            0.5 / 255.0,
            1.5 / 255.0,
            0.25,
            0.5,
            0.75,
            1.0,
            2.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            0.333,
            0.666,
            0.999,
        ] {
            push(value);
        }
        // 16 values = 4 pixels; add one more pixel so the scalar tail runs too.
        for value in [0.1f32, 0.2, 0.3, 0.4] {
            push(value);
        }
        let pixels = source.len() / 8;
        let mut expected = vec![0u8; pixels * 4];
        unpack_row_scalar(&source, &mut expected);
        let mut actual = vec![0u8; pixels * 4];
        unsafe { unpack_row_f16c(&source, &mut actual) };
        assert_eq!(actual, expected);
    }

    /// A bare number in the log makes a reader look the code up; the ones AE
    /// defines carry their name (issue #697). Codes outside the enum - a
    /// plug-in's own, the host's negative internal ones - keep just the number
    /// rather than being given a name they do not have.
    #[test]
    fn known_pf_errors_are_named_and_unknown_ones_are_not() {
        assert_eq!(pf_error_name(4), Some("PF_Err_OUT_OF_MEMORY"));
        assert_eq!(pf_error_name(512), Some("PF_Err_INTERNAL_STRUCT_DAMAGED"));
        assert_eq!(
            pf_error_name(518),
            Some("PF_Err_CANNOT_PARSE_KEYFRAME_TEXT")
        );
        assert_eq!(pf_error_name(0), None);
        assert_eq!(pf_error_name(-3), None);
        assert_eq!(pf_error_name(519), None);
    }

    /// One line when the trouble starts, then one every
    /// `FRAME_TROUBLE_REPORT_INTERVAL` frames: an effect failing at the
    /// preview's frame rate must not bury the log it exists to make readable.
    #[test]
    fn a_run_of_identical_frame_errors_reports_once_then_periodically() {
        let plugin = PathBuf::from(r"C:\plugins\Displacement.aex");
        let mut states = FrameTroubleStates::new();
        let lines: Vec<String> = (0..FRAME_TROUBLE_REPORT_INTERVAL)
            .filter_map(|_| {
                frame_trouble_report(&mut states, &plugin, FrameTrouble::Error(4, None))
            })
            .collect();
        assert_eq!(
            lines.len(),
            2,
            "the first frame and the interval mark, not one per frame: {lines:?}"
        );
        assert!(lines[0].contains("Displacement"), "{lines:?}");
        assert!(lines[0].contains("PF_Err_OUT_OF_MEMORY"), "{lines:?}");
        assert!(
            lines[1].contains(&format!("x{FRAME_TROUBLE_REPORT_INTERVAL}")),
            "the repeat says how many frames it covers: {lines:?}"
        );
    }

    /// The recovery is reported once, and only for a filter that was reported
    /// as failing: a healthy one stays silent.
    #[test]
    fn a_recovery_is_reported_once_and_only_after_trouble() {
        let plugin = PathBuf::from(r"C:\plugins\Displacement.aex");
        let mut states = FrameTroubleStates::new();
        assert!(frame_recovered_report(&mut states, &plugin).is_none());
        frame_trouble_report(&mut states, &plugin, FrameTrouble::Error(4, None));
        frame_trouble_report(&mut states, &plugin, FrameTrouble::Error(4, None));
        let recovered = frame_recovered_report(&mut states, &plugin)
            .expect("a filter that was failing says when it stops");
        assert!(recovered.contains("rendering again"), "{recovered}");
        assert!(recovered.contains("2 frame(s)"), "{recovered}");
        assert!(frame_recovered_report(&mut states, &plugin).is_none());
    }

    /// The plug-in's own words reach the log - "PF_Err_INTERNAL_STRUCT_DAMAGED"
    /// alone does not tell a user that a suite could not be acquired - but they
    /// stay out of the identity the run is collapsed on, so a plug-in that
    /// varies its message per frame cannot defeat the interval (issue #707).
    #[test]
    fn a_plug_ins_own_reason_reaches_the_log_without_defeating_the_interval() {
        let plugin = PathBuf::from(r"C:\plugins\Invert.aex");
        let mut states = FrameTroubleStates::new();
        let first = frame_trouble_report(
            &mut states,
            &plugin,
            FrameTrouble::Error(512, Some("Couldn't load suite.")),
        )
        .expect("the first trouble is reported");
        assert!(first.contains("Invert"), "{first}");
        assert!(first.contains("PF_Err_INTERNAL_STRUCT_DAMAGED"), "{first}");
        assert!(first.contains("Couldn't load suite."), "{first}");

        // A plug-in numbering its message per frame must not get a line per
        // frame: the identity is the code, so the run still collapses.
        let noisy: Vec<String> = (1..FRAME_TROUBLE_REPORT_INTERVAL)
            .filter_map(|frame| {
                let text = format!("bad sample at t={frame}");
                frame_trouble_report(&mut states, &plugin, FrameTrouble::Error(512, Some(&text)))
            })
            .collect();
        assert_eq!(
            noisy.len(),
            1,
            "only the interval mark, not one line per frame: {noisy:?}"
        );
        // The line that is emitted still carries the words from that frame.
        assert!(noisy[0].contains("bad sample at t="), "{}", noisy[0]);
        assert!(
            noisy[0].contains(&format!("x{FRAME_TROUBLE_REPORT_INTERVAL}")),
            "{}",
            noisy[0]
        );

        // Saying nothing is the ordinary case and reads exactly as before.
        let mut quiet = FrameTroubleStates::new();
        let silent = frame_trouble_report(&mut quiet, &plugin, FrameTrouble::Error(512, None))
            .expect("the first trouble is reported");
        assert_eq!(
            silent,
            "Invert: frame error 512 (PF_Err_INTERNAL_STRUCT_DAMAGED)"
        );
    }

    /// A different reason for the same filter is its own line: collapsing an
    /// out-of-memory run and a lost session into one count would hide the
    /// change of failure.
    #[test]
    fn a_changed_reason_starts_its_own_report() {
        let plugin = PathBuf::from(r"C:\plugins\Blend.aex");
        let mut states = FrameTroubleStates::new();
        let lines: Vec<String> = [
            FrameTrouble::Error(4, None),
            FrameTrouble::Error(4, None),
            FrameTrouble::SessionLost("worker exited"),
        ]
        .into_iter()
        .filter_map(|trouble| frame_trouble_report(&mut states, &plugin, trouble))
        .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[1].contains("worker exited"), "{lines:?}");
    }

    // --- settings dialog (issue #855): the text⇄config⇄TOML mapping ---------

    fn full_form() -> ConfigForm {
        ConfigForm {
            dirs: "C:\\plugins\r\n  D:\\more \r\n\r\n".into(),
            dependency_dirs: "C:\\ae\\Support Files".into(),
            ignore: "Noisy.aex\r\nSlow".into(),
            repository: "  C:\\repo  ".into(),
            module_limit: " 40 ".into(),
            byte_limit: "1073741824".into(),
        }
    }

    /// Blank lines and padding are user typing, not config: entries are the
    /// trimmed non-empty lines.
    #[test]
    fn form_lines_are_trimmed_and_blank_lines_dropped() {
        let edit = parse_form(&full_form()).expect("a fully valid form parses");
        assert_eq!(edit.dirs, vec!["C:\\plugins", "D:\\more"]);
        assert_eq!(edit.ignore, vec!["Noisy.aex", "Slow"]);
        assert_eq!(edit.repository.as_deref(), Some("C:\\repo"));
        assert_eq!(edit.dependency_module_limit, Some(40));
        assert_eq!(edit.dependency_byte_limit, Some(1 << 30));
    }

    /// Empty text fields mean "no value", matching the absent TOML keys they
    /// map to (no ceiling, default folders, default worker resolution).
    #[test]
    fn an_empty_form_is_all_defaults() {
        let edit = parse_form(&ConfigForm::default()).expect("an empty form is valid");
        assert!(edit.dirs.is_empty());
        assert_eq!(edit.repository, None);
        assert_eq!(edit.dependency_module_limit, None);
        assert_eq!(edit.dependency_byte_limit, None);
    }

    /// A limit that does not parse is a user error to show, not a value to
    /// guess at or silently drop.
    #[test]
    fn a_malformed_limit_is_rejected_with_its_text() {
        let form = ConfigForm {
            module_limit: "many".into(),
            ..ConfigForm::default()
        };
        let error = parse_form(&form).expect_err("'many' is not a count");
        assert!(error.contains("many"), "{error}");
    }

    /// TOML integers are i64: a limit beyond that would be written as a
    /// wrapped negative that the next launch's `load_config` rejects — a
    /// saved-then-ignored config — so it is rejected at the dialog instead.
    #[test]
    fn a_byte_limit_beyond_toml_range_is_rejected() {
        let form = ConfigForm {
            byte_limit: u64::MAX.to_string(),
            ..ConfigForm::default()
        };
        parse_form(&form).expect_err("u64::MAX does not fit a TOML integer");
    }

    /// Same for the module count (usize is also 64-bit here).
    #[test]
    fn a_module_limit_beyond_toml_range_is_rejected() {
        let form = ConfigForm {
            module_limit: u64::MAX.to_string(),
            ..ConfigForm::default()
        };
        parse_form(&form).expect_err("u64::MAX does not fit a TOML integer");
    }

    /// The dialog shows what the file says: `dir` and `dirs` fold together,
    /// and absent values are empty fields.
    #[test]
    fn the_form_mirrors_the_config_file() {
        let config = Config {
            dir: Some(PathBuf::from("C:\\single")),
            dirs: vec![PathBuf::from("C:\\more")],
            repository: None,
            dependency_dirs: Vec::new(),
            dependency_module_limit: Some(7),
            dependency_byte_limit: None,
            ignore: vec!["Noisy".into()],
        };
        let form = form_from_config(&config);
        assert_eq!(form.dirs, "C:\\single\r\nC:\\more");
        assert_eq!(form.repository, "");
        assert_eq!(form.module_limit, "7");
        assert_eq!(form.byte_limit, "");
        assert_eq!(form.ignore, "Noisy");
    }

    /// A dialog save must not destroy what it does not manage: comments and
    /// unknown keys in a hand-written config survive, and the round-tripped
    /// text still parses to the values that were saved.
    #[test]
    fn a_save_preserves_comments_and_unknown_keys() {
        let existing = "# my notes\nfuture_key = true\ndir = 'C:\\old'\nrepository = 'C:\\repo'\n";
        let edit = parse_form(&full_form()).expect("a fully valid form parses");
        let (text, backed_up) = merged_config_text(existing, &edit);
        assert!(!backed_up, "a parseable file is merged, not replaced");
        assert!(text.contains("# my notes"), "{text}");
        assert!(text.contains("future_key = true"), "{text}");
        assert!(!text.contains("C:\\old"), "`dir` folds into `dirs`: {text}");
        let reloaded: toml::Value = toml::from_str(&text).expect("the written file parses");
        assert_eq!(
            reloaded["dirs"],
            toml::Value::Array(vec!["C:\\plugins".into(), "D:\\more".into()]),
            "{text}"
        );
        assert_eq!(reloaded["dependency_module_limit"], toml::Value::Integer(40));
    }

    /// Clearing a field removes its key: an absent key already means "use the
    /// default", and a lingering stale value would override it.
    #[test]
    fn a_cleared_field_removes_its_key() {
        let existing =
            "dirs = ['C:\\plugins']\nrepository = 'C:\\repo'\ndependency_byte_limit = 9\n";
        let edit = parse_form(&ConfigForm::default()).expect("an empty form is valid");
        let (text, backed_up) = merged_config_text(existing, &edit);
        assert!(!backed_up);
        let reloaded: toml::Value = toml::from_str(&text).expect("the written file parses");
        let table = reloaded.as_table().expect("a TOML document is a table");
        assert!(table.is_empty(), "every managed key was removed: {text}");
    }

    /// An unparseable existing file cannot be merged into; the save still
    /// succeeds against an empty document, and the caller is told so it backs
    /// the original up first.
    #[test]
    fn an_unparseable_file_is_flagged_for_backup() {
        let edit = parse_form(&full_form()).expect("a fully valid form parses");
        let (text, backed_up) = merged_config_text("not toml [", &edit);
        assert!(backed_up);
        toml::from_str::<toml::Value>(&text).expect("the replacement parses");
    }

    /// What the dialog writes is what `load_config` reads: the full form
    /// round-trips through the real `Config` deserialization.
    #[test]
    fn a_saved_form_round_trips_through_config() {
        let edit = parse_form(&full_form()).expect("a fully valid form parses");
        let (text, _) = merged_config_text("", &edit);
        let config: Config = toml::from_str(&text).expect("the written file parses as Config");
        assert_eq!(config.dir, None);
        assert_eq!(
            config.dirs,
            vec![PathBuf::from("C:\\plugins"), PathBuf::from("D:\\more")]
        );
        assert_eq!(config.repository, Some(PathBuf::from("C:\\repo")));
        assert_eq!(config.dependency_byte_limit, Some(1 << 30));
        assert_eq!(config.ignore, vec!["Noisy.aex", "Slow"]);
        // And the reload shows in the dialog what was typed (modulo trimming).
        let form = form_from_config(&config);
        assert_eq!(form.dirs, "C:\\plugins\r\nD:\\more");
        assert_eq!(form.module_limit, "40");
    }

    fn save_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aexcompat-multifilter-save-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A first save creates the folder and the file, leaves no staging file
    /// behind, and reports no backup.
    #[test]
    fn a_save_creates_the_config_file() {
        let dir = save_dir("fresh");
        let path = dir.join("config.toml");
        let edit = parse_form(&full_form()).expect("a fully valid form parses");
        let backup = write_config_edit(&path, &edit).expect("saving into a new folder works");
        assert_eq!(backup, None);
        let config: Config =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).expect("the file loads");
        assert_eq!(config.dependency_module_limit, Some(40));
        assert!(
            !path.with_extension("toml.tmp").exists(),
            "the staging file was renamed away"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An unparseable existing file is preserved in the reported backup, and
    /// the config itself is rewritten cleanly.
    #[test]
    fn a_save_over_a_broken_file_backs_it_up() {
        let dir = save_dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "not toml [").unwrap();
        let edit = parse_form(&ConfigForm::default()).expect("an empty form is valid");
        let backup = write_config_edit(&path, &edit)
            .expect("a broken file is backed up, not a failure")
            .expect("the broken file was flagged");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "not toml [",
            "the original bytes survive in the backup"
        );
        toml::from_str::<Config>(&std::fs::read_to_string(&path).unwrap())
            .expect("the rewritten file loads");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file that exists but cannot be read must not be overwritten: losing
    /// unreadable content is worse than failing the save.
    #[test]
    fn an_unreadable_file_refuses_the_save() {
        let dir = save_dir("unreadable");
        // A directory at the config path reads as an error that is not
        // NotFound, standing in for a locked/permission-broken file.
        std::fs::create_dir_all(dir.join("config.toml")).unwrap();
        let edit = parse_form(&ConfigForm::default()).expect("an empty form is valid");
        write_config_edit(&dir.join("config.toml"), &edit)
            .expect_err("an unreadable existing file blocks the save");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // --- the ignore checklist (issue #858) ----------------------------------

    fn known(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    /// Two same-stem AEX in different folders are one ignore decision (the
    /// list matches by stem, case-insensitively), and rows come out sorted.
    #[test]
    fn ignore_rows_dedupe_stems_across_folders() {
        let known = known(&[
            r"C:\a\Tint.aex",
            r"C:\b\TINT.aex",
            r"C:\a\Blur.aex",
            r"C:\cache-only\Gone.aex",
        ]);
        let (rows, manual) = ignore_rows(&known, &[]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Blur", "Gone", "Tint"]);
        assert!(rows.iter().all(|row| !row.ignored));
        assert!(manual.is_empty());
    }

    /// The checkbox state mirrors the ignore list under its own matching
    /// rules: case-insensitive, `.aex` optional.
    #[test]
    fn ignore_rows_check_matching_entries() {
        let known = known(&[r"C:\a\Tint.aex", r"C:\a\Blur.aex"]);
        let ignore = vec!["tint.AEX".to_owned(), "Sharpen".to_owned()];
        let (rows, manual) = ignore_rows(&known, &ignore);
        assert_eq!(
            rows,
            vec![
                IgnoreRow {
                    name: "Blur".into(),
                    ignored: false
                },
                IgnoreRow {
                    name: "Tint".into(),
                    ignored: true
                },
            ]
        );
        // An entry no known effect matches survives via the manual field; it
        // may name an effect that simply is not visible this launch.
        assert_eq!(manual, ["Sharpen"]);
    }

    /// The saved list is checked rows plus manual lines, without letting a
    /// manual respelling duplicate a checked effect.
    #[test]
    fn compose_ignore_merges_checks_and_manual_lines() {
        let ignore = compose_ignore(
            vec!["Tint".into(), "Blur".into()],
            "Sharpen\r\n tint.aex \r\nBlur",
        );
        assert_eq!(ignore, ["Tint", "Blur", "Sharpen"]);
    }

    /// The template stream is what keeps the dialog alive at all; a malformed
    /// header (wrong item count, odd alignment) fails at runtime inside
    /// AviUtl2, so pin the invariants that are checkable here.
    #[test]
    fn the_dialog_template_is_well_formed() {
        let template = build_dialog_template();
        let words: &[u16] = unsafe {
            std::slice::from_raw_parts(template.as_ptr().cast(), template.len() * 2)
        };
        // Header: style, exstyle, then the item count at u16 index 4.
        let style = (words[0] as u32) | ((words[1] as u32) << 16);
        assert_ne!(style & DS_SETFONT, 0, "the font block below is only read with DS_SETFONT");
        let declared = words[4] as usize;
        // Count the DLGITEMTEMPLATE headers by walking the stream: after the
        // header (menu=0, class=0, title, pointsize, face), each item starts
        // DWORD-aligned with style|WS_CHILD|WS_VISIBLE.
        let mut found = 0;
        let mut at = 5 + 4; // past style/exstyle/cdit + x,y,cx,cy
        assert_eq!(words[at], 0, "no menu");
        assert_eq!(words[at + 1], 0, "default dialog class");
        at += 2;
        while words[at] != 0 {
            at += 1; // title
        }
        at += 1;
        at += 1; // point size
        while words[at] != 0 {
            at += 1; // font face
        }
        at += 1;
        while found < declared {
            if at % 2 != 0 {
                at += 1;
            }
            let item_style = (words[at] as u32) | ((words[at + 1] as u32) << 16);
            assert_ne!(item_style & WS_CHILD, 0, "item {found} carries WS_CHILD");
            at += 4 + 4 + 1; // style, exstyle, rect, id
            if words[at] == 0xFFFF {
                at += 2; // class atom
            } else {
                assert_ne!(words[at], 0, "item {found} has a class (atom or name)");
                while words[at] != 0 {
                    at += 1; // class name (the SysListView32 checklist)
                }
                at += 1;
            }
            while words[at] != 0 {
                at += 1; // title
            }
            at += 1;
            assert_eq!(words[at], 0, "item {found} has no creation data");
            at += 1;
            found += 1;
        }
        assert_eq!(found, declared);
        assert!(at <= words.len());
    }
}
