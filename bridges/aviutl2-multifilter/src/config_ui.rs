// Settings dialog (issue #855): edit the TOML config from inside AviUtl2.
//
// `register_config_menu` puts an entry in AviUtl2's settings menu; picking it
// calls [`config_menu_entry`], which shows a modal Win32 dialog over the values
// currently in `config.toml` and writes edits back through `toml_edit`, so
// comments and unknown keys in a hand-written file survive a dialog save. (A
// comment attached to a value the dialog rewrites goes with the value; what
// survives is everything the dialog does not manage.)
//
// The dialog edits the FILE, not the running plug-in: the config is read once
// at `RegisterPlugin` and AviUtl2 freezes every filter's config set at load, so
// a save takes effect on the next launch. The dialog says so on its face and
// again after a save. Env overrides (`AEXCOMPAT_MULTIFILTER_*`) keep beating
// the file at load, so an active override is called out in the dialog rather
// than silently making a saved value appear to do nothing.

use windows_sys::Win32::Foundation::{
    HINSTANCE as WsHINSTANCE, HWND as WsHWND, LPARAM, RECT, WPARAM,
};
use windows_sys::Win32::UI::Controls::{
    ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_WIDTH, LVCOLUMNW,
    LVIF_TEXT, LVIS_STATEIMAGEMASK, LVITEMW, LVM_GETITEMCOUNT, LVM_GETITEMSTATE, LVM_INSERTCOLUMNW,
    LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMSTATE, LVS_EX_CHECKBOXES,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DialogBoxIndirectParamW, EndDialog, GetClientRect, GetDlgItem, GetSystemMetrics,
    GetWindowTextLengthW, GetWindowTextW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW,
    SM_CXVSCROLL, SendMessageW, SetDlgItemTextW, SetWindowLongPtrW, WM_COMMAND, WM_INITDIALOG,
};

/// The values shown in (and read back from) the dialog, all as text. List
/// fields hold one entry per line. A pure model, so the text⇄config mapping is
/// testable without a window.
#[derive(Default, PartialEq, Debug)]
struct ConfigForm {
    /// `dir` + `dirs` folded together (the dialog has no reason to preserve the
    /// single-folder spelling; a save rewrites it as `dirs`).
    dirs: String,
    dependency_dirs: String,
    ignore: String,
    repository: String,
    /// Empty means "no ceiling", matching the absent TOML key.
    module_limit: String,
    byte_limit: String,
}

/// A validated edit, ready to merge into the TOML document.
#[derive(PartialEq, Debug)]
struct ConfigEdit {
    dirs: Vec<String>,
    dependency_dirs: Vec<String>,
    ignore: Vec<String>,
    repository: Option<String>,
    dependency_module_limit: Option<usize>,
    dependency_byte_limit: Option<u64>,
}

/// One known effect in the ignore checklist (issue #858).
#[derive(PartialEq, Debug)]
struct IgnoreRow {
    /// The file stem the ignore list matches on, in a spelling seen on disk.
    name: String,
    /// Whether the current ignore list matches it (= the checkbox state).
    ignored: bool,
}

/// The checklist rows for `known` effect paths under the current `ignore`
/// list, plus the ignore entries no known effect matches (which go to the
/// manual field — dropping them would silently un-ignore an effect that is
/// merely not visible this launch, and #307's rule is that absence is not
/// evidence). Rows are deduplicated by stem (ignore matching is stem-based
/// and case-insensitive, so two same-stem AEX in different folders are one
/// decision) and sorted for the display.
fn ignore_rows(known: &[PathBuf], ignore: &[String]) -> (Vec<IgnoreRow>, Vec<String>) {
    let mut stems: Vec<&str> = known
        .iter()
        .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()))
        .collect();
    stems.sort_by_key(|stem| stem.to_ascii_lowercase());
    stems.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let rows: Vec<IgnoreRow> = stems
        .into_iter()
        .map(|stem| IgnoreRow {
            name: stem.to_owned(),
            ignored: ignore
                .iter()
                .any(|entry| strip_aex_ext(entry).eq_ignore_ascii_case(stem)),
        })
        .collect();
    let manual: Vec<String> = ignore
        .iter()
        .filter(|entry| {
            !rows
                .iter()
                .any(|row| strip_aex_ext(entry).eq_ignore_ascii_case(&row.name))
        })
        .cloned()
        .collect();
    (rows, manual)
}

/// The saved ignore list: every checked row plus the manual lines, minus
/// duplicates (a manual line naming a checked effect, with or without the
/// `.aex` spelling).
fn compose_ignore(checked: Vec<String>, manual: &str) -> Vec<String> {
    let mut ignore = checked;
    for entry in parse_lines(manual) {
        if !ignore
            .iter()
            .any(|kept| strip_aex_ext(kept).eq_ignore_ascii_case(strip_aex_ext(&entry)))
        {
            ignore.push(entry);
        }
    }
    ignore
}

/// Every AEX the checklist can offer: what the current config's folders hold
/// right now (ignored ones included — they are exactly what the list is for)
/// plus everything the discovery cache remembers (an effect missing this
/// launch can still be un-ignored).
///
/// Runs on the UI thread when the dialog opens. The walk is the same one
/// `RegisterPlugin` already does on the load thread every launch, so a folder
/// set slow enough to stall the dialog stalls every startup first.
fn known_effects(config: &Config) -> Vec<PathBuf> {
    let (dirs, _) = resolve_scan_dirs(config);
    let mut known = collect_aex(&dirs, &[]).seen;
    known.extend(load_cache().keys().map(PathBuf::from));
    known
}

/// One trimmed, non-empty entry per line (either line ending).
fn parse_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn join_lines<S: AsRef<str>>(entries: impl IntoIterator<Item = S>) -> String {
    let mut out = String::new();
    for entry in entries {
        if !out.is_empty() {
            out.push_str("\r\n");
        }
        out.push_str(entry.as_ref());
    }
    out
}

/// The dialog's initial contents, from the config as the file spells it (env
/// overrides excluded — the dialog edits the file, and the overrides are
/// reported separately by [`env_override_note`]).
fn form_from_config(config: &Config) -> ConfigForm {
    let dirs = config.dir.iter().chain(&config.dirs);
    ConfigForm {
        dirs: join_lines(dirs.map(|dir| dir.to_string_lossy())),
        dependency_dirs: join_lines(config.dependency_dirs.iter().map(|d| d.to_string_lossy())),
        ignore: join_lines(&config.ignore),
        repository: config
            .repository
            .as_deref()
            .map(|repo| repo.to_string_lossy().into_owned())
            .unwrap_or_default(),
        module_limit: config
            .dependency_module_limit
            .map(|limit| limit.to_string())
            .unwrap_or_default(),
        byte_limit: config
            .dependency_byte_limit
            .map(|limit| limit.to_string())
            .unwrap_or_default(),
    }
}

/// Validates the dialog text into an edit. Error strings face the user (the
/// dialog is Japanese-labelled, so these are too).
fn parse_form(form: &ConfigForm) -> Result<ConfigEdit, String> {
    let module_limit = parse_limit::<usize>(&form.module_limit, "依存 DLL 数の上限")?;
    let byte_limit = parse_limit::<u64>(&form.byte_limit, "依存 DLL 合計バイト数の上限")?;
    // TOML integers are i64; a larger value would be written as a wrapped
    // negative, and the next launch's `load_config` would then reject the
    // whole file — a "saved" config that ignores every setting (issue #655's
    // shape, created by the dialog itself).
    if module_limit.is_some_and(|limit| limit as u64 > i64::MAX as u64) {
        return Err(format!(
            "依存 DLL 数の上限が大きすぎます ({} 以下)",
            i64::MAX
        ));
    }
    if byte_limit.is_some_and(|limit| limit > i64::MAX as u64) {
        return Err(format!(
            "依存 DLL 合計バイト数の上限が大きすぎます ({} 以下)",
            i64::MAX
        ));
    }
    let repository = form.repository.trim();
    Ok(ConfigEdit {
        dirs: parse_lines(&form.dirs),
        dependency_dirs: parse_lines(&form.dependency_dirs),
        ignore: parse_lines(&form.ignore),
        repository: (!repository.is_empty()).then(|| repository.to_owned()),
        dependency_module_limit: module_limit,
        dependency_byte_limit: byte_limit,
    })
}

fn parse_limit<T: std::str::FromStr>(text: &str, label: &str) -> Result<Option<T>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<T>()
        .map(Some)
        .map_err(|_| format!("{label}が数値として読めません: {text}"))
}

/// Merges the edit into the existing TOML text, keeping comments and keys the
/// dialog does not know about. An empty list/absent value removes its key (an
/// absent key already means "use the default", and a `dirs = []` would say the
/// same thing more confusingly). The single-folder `dir` spelling is folded
/// into `dirs` by [`form_from_config`], so it is always removed.
fn apply_edit(existing: &str, edit: &ConfigEdit) -> Result<String, toml_edit::TomlError> {
    let mut doc: toml_edit::DocumentMut = existing.parse()?;
    doc.remove("dir");
    set_string_array(&mut doc, "dirs", &edit.dirs);
    set_string_array(&mut doc, "dependency_dirs", &edit.dependency_dirs);
    set_string_array(&mut doc, "ignore", &edit.ignore);
    set_optional(&mut doc, "repository", edit.repository.as_deref());
    set_optional(
        &mut doc,
        "dependency_module_limit",
        edit.dependency_module_limit.map(|limit| limit as i64),
    );
    set_optional(
        &mut doc,
        "dependency_byte_limit",
        edit.dependency_byte_limit.map(|limit| limit as i64),
    );
    Ok(doc.to_string())
}

fn set_string_array(doc: &mut toml_edit::DocumentMut, key: &str, entries: &[String]) {
    if entries.is_empty() {
        doc.remove(key);
        return;
    }
    let array: toml_edit::Array = entries.iter().collect();
    doc[key] = toml_edit::value(array);
}

fn set_optional<V: Into<toml_edit::Value>>(
    doc: &mut toml_edit::DocumentMut,
    key: &str,
    value: Option<V>,
) {
    match value {
        Some(value) => doc[key] = toml_edit::value(value),
        None => {
            doc.remove(key);
        }
    }
}

/// The new file text, plus whether the existing text had to be abandoned
/// because it does not parse (the caller then backs the original up rather
/// than silently destroying whatever it was).
fn merged_config_text(existing: &str, edit: &ConfigEdit) -> (String, bool) {
    match apply_edit(existing, edit) {
        Ok(text) => (text, false),
        Err(_) => {
            let text = apply_edit("", edit)
                .expect("an empty TOML document parses; merging into it cannot fail");
            (text, true)
        }
    }
}

/// Writes the edit to the config file ([`config_path`], so an
/// `AEXCOMPAT_MULTIFILTER_CONFIG` override is honoured). Returns the path
/// written and the backup path when the previous file was unparseable.
fn save_config_edit(edit: &ConfigEdit) -> Result<(PathBuf, Option<PathBuf>), String> {
    let path = config_path()
        .ok_or_else(|| "APPDATA が取得できないため、保存先を決定できません".to_owned())?;
    let backup = write_config_edit(&path, edit)?;
    if let Some(backup) = &backup {
        log_warn(&format!(
            "{} could not be parsed; the settings dialog backed it up to {} before rewriting it",
            path.display(),
            backup.display()
        ));
    }
    log_info(&format!(
        "the settings dialog wrote {}; the changes apply on the next AviUtl2 launch",
        path.display()
    ));
    Ok((path, backup))
}

/// The file half of a save, separated from [`config_path`] resolution and the
/// host logging so the backup/staged-write behaviour is testable against a
/// plain path (the log buffer is process-global state a test must not touch).
fn write_config_edit(path: &Path, edit: &ConfigEdit) -> Result<Option<PathBuf>, String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        // Refuse rather than overwrite content that could not even be read.
        Err(error) => {
            return Err(format!(
                "{} が読み取れないため、上書きを中止しました: {error}",
                path.display()
            ));
        }
    };
    let (text, backed_up) = merged_config_text(&existing, edit);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{} を作成できません: {error}", parent.display()))?;
    }
    let backup = backed_up.then(|| path.with_extension("toml.bak"));
    if let Some(backup) = &backup {
        std::fs::copy(path, backup)
            .map_err(|error| format!("{} へ退避できません: {error}", backup.display()))?;
    }
    // Write-then-rename, so a crash mid-save cannot leave a half-written config
    // (std's rename replaces the destination on Windows).
    let staged = path.with_extension("toml.tmp");
    std::fs::write(&staged, &text)
        .map_err(|error| format!("{} に書き込めません: {error}", staged.display()))?;
    std::fs::rename(&staged, path)
        .map_err(|error| format!("{} に書き込めません: {error}", path.display()))?;
    Ok(backup)
}

/// Lines describing env vars that will beat whatever the dialog saves, so a
/// user who set (or inherited) one is not left wondering why a saved value
/// changes nothing. Empty when no override is active.
fn env_override_note() -> String {
    let overrides: [(&str, &str); 4] = [
        (ENV_DIR, "スキャンフォルダを上書きしています"),
        (ENV_DEPENDENCY_DIRS, "依存 DLL フォルダを上書きしています"),
        (ENV_REPOSITORY, "worker リポジトリを上書きしています"),
        (ENV_CONFIG, "設定ファイルの場所を変更しています"),
    ];
    join_lines(
        overrides
            .iter()
            .filter(|(name, _)| std::env::var_os(name).is_some())
            .map(|(name, what)| format!("注意: 環境変数 {name} が{what}")),
    )
}

// --- Win32 dialog ---------------------------------------------------------

const IDC_DIRS: u16 = 1001;
const IDC_DEPENDENCY_DIRS: u16 = 1002;
const IDC_IGNORE: u16 = 1003;
const IDC_REPOSITORY: u16 = 1004;
const IDC_MODULE_LIMIT: u16 = 1005;
const IDC_BYTE_LIMIT: u16 = 1006;
const IDC_ENV_NOTE: u16 = 1007;
const IDC_IGNORE_LIST: u16 = 1008;
const IDC_OK: u16 = 1; // IDOK
const IDC_CANCEL: u16 = 2; // IDCANCEL

/// The dialog's per-window slot for application data. WinUser.h defines it as
/// `DWLP_DLGPROC + sizeof(DLGPROC)` where `DWLP_DLGPROC` is
/// `DWLP_MSGRESULT + sizeof(LRESULT)` — two pointer-sized slots in; windows-sys
/// does not carry the derived constant.
const DWLP_USER: i32 = 2 * std::mem::size_of::<isize>() as i32;

/// What `WM_INITDIALOG` fills the controls from — and what the save reads the
/// checklist rows back through (the list control holds check states by index;
/// the names live here). Passed by reference through `dwInitParam` and stashed
/// in `DWLP_USER`; the dialog is modal, so the caller's frame outlives it.
struct DialogInit {
    form: ConfigForm,
    env_note: String,
    rows: Vec<IgnoreRow>,
}

/// Registers the settings-menu entry. Called from `RegisterPlugin` before any
/// early return: the dialog is how a user fixes the very configuration whose
/// absence causes those early returns.
fn register_settings_menu(host: *mut HOST_APP_TABLE) {
    // SAFETY: the caller checked `host` for null; the name is leaked for the
    // host's lifetime as every other registration string is.
    unsafe {
        ((*host).register_config_menu)(wide_leak("AEXCompat multi-filter"), config_menu_entry);
    }
}

/// The settings-menu callback. A panic must not unwind into AviUtl2.
unsafe extern "C" fn config_menu_entry(
    parent: aviutl2_sys::plugin2::HWND,
    instance: aviutl2_sys::plugin2::HINSTANCE,
) {
    let outcome = std::panic::catch_unwind(|| show_config_dialog(parent, instance));
    if outcome.is_err() {
        log_warn("the settings dialog panicked; the config file was not changed");
    }
}

fn show_config_dialog(parent: WsHWND, instance: WsHINSTANCE) {
    let config = load_config();
    let (rows, manual) = ignore_rows(&known_effects(&config), &config.ignore);
    let mut form = form_from_config(&config);
    // The known effects moved into the checklist; the text field keeps only
    // the entries the checklist cannot express.
    form.ignore = join_lines(&manual);
    let init = DialogInit {
        form,
        env_note: env_override_note(),
        rows,
    };
    // The checklist is a common control; registering its class is idempotent.
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES,
    };
    // SAFETY: the struct is initialized and alive for the call.
    unsafe { InitCommonControlsEx(&icc) };
    let template = build_dialog_template();
    // SAFETY: the template buffer and `init` outlive the call (the dialog is
    // modal); the dialog proc matches the DLGPROC contract.
    let outcome = unsafe {
        DialogBoxIndirectParamW(
            instance,
            template.as_ptr().cast(),
            parent,
            Some(config_dialog_proc),
            &init as *const DialogInit as LPARAM,
        )
    };
    // 1 = saved, 2 = cancelled (the EndDialog results below); 0/-1 mean the
    // dialog never came up, which from the menu looks like a dead click — say
    // so rather than joining the silent-failure modes issue #655 removed.
    if outcome <= 0 {
        log_warn(&format!(
            "the settings dialog could not be created (DialogBoxIndirectParamW returned {outcome})"
        ));
    }
}

unsafe extern "system" fn config_dialog_proc(
    dialog: WsHWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    // A panic in the proc must not unwind into the dialog manager. Claiming
    // "not handled" (0) is safe for every message this proc looks at.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_dialog_message(dialog, message, wparam, lparam)
    }))
    .unwrap_or(0)
}

fn handle_dialog_message(dialog: WsHWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    match message {
        WM_INITDIALOG => {
            // SAFETY: `lparam` is the `&DialogInit` passed to
            // `DialogBoxIndirectParamW` above, alive for the modal call.
            let init = unsafe { &*(lparam as *const DialogInit) };
            // Stashed so the save can pair the checklist's by-index states
            // with their names. SAFETY: `dialog` is this proc's live dialog.
            unsafe { SetWindowLongPtrW(dialog, DWLP_USER, lparam) };
            set_item_text(dialog, IDC_DIRS, &init.form.dirs);
            set_item_text(dialog, IDC_DEPENDENCY_DIRS, &init.form.dependency_dirs);
            set_item_text(dialog, IDC_IGNORE, &init.form.ignore);
            set_item_text(dialog, IDC_REPOSITORY, &init.form.repository);
            set_item_text(dialog, IDC_MODULE_LIMIT, &init.form.module_limit);
            set_item_text(dialog, IDC_BYTE_LIMIT, &init.form.byte_limit);
            set_item_text(dialog, IDC_ENV_NOTE, &init.env_note);
            fill_ignore_list(dialog, &init.rows);
            1
        }
        WM_COMMAND => match (wparam & 0xFFFF) as u16 {
            IDC_OK => {
                if save_from_dialog(dialog) {
                    // SAFETY: `dialog` is this proc's own live dialog.
                    unsafe { EndDialog(dialog, 1) };
                }
                1
            }
            IDC_CANCEL => {
                // Also delivered for ESC and the title-bar close button. 2,
                // not 0: `DialogBoxIndirectParamW` returns 0/-1 for "the
                // dialog never ran", and a cancel must not look like that.
                // SAFETY: as above.
                unsafe { EndDialog(dialog, 2) };
                1
            }
            _ => 0,
        },
        _ => 0,
    }
}

/// The state stashed at `WM_INITDIALOG`, or `None` if it never ran.
fn dialog_init(dialog: WsHWND) -> Option<&'static DialogInit> {
    // SAFETY: the slot holds the `&DialogInit` stored at WM_INITDIALOG (or 0),
    // and the pointee outlives the modal dialog. `'static` overstates the
    // lifetime but the reference never escapes the dialog's callbacks.
    unsafe {
        let stored =
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(dialog, DWLP_USER);
        (stored != 0).then(|| &*(stored as *const DialogInit))
    }
}

/// Reads the controls back, validates, and saves. `false` keeps the dialog
/// open — after a message box saying why, except in the init-never-ran case,
/// which cannot be reached by clicking OK on a dialog that never initialized.
fn save_from_dialog(dialog: WsHWND) -> bool {
    let Some(init) = dialog_init(dialog) else {
        return false;
    };
    // A checklist holding fewer rows than it was given (a failed insert; see
    // `fill_ignore_list`) reads every shifted row as unchecked, which would
    // save a silently un-ignored effect. Refuse instead.
    if ignore_list_count(dialog) != Some(init.rows.len()) {
        message_box(
            dialog,
            "無視リストの表示が壊れているため保存を中止しました。ダイアログを開き直してください。",
            MB_ICONERROR,
        );
        return false;
    }
    let checked: Vec<String> = init
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| ignore_list_checked(dialog, *index))
        .map(|(_, row)| row.name.clone())
        .collect();
    let form = ConfigForm {
        dirs: item_text(dialog, IDC_DIRS),
        dependency_dirs: item_text(dialog, IDC_DEPENDENCY_DIRS),
        ignore: join_lines(compose_ignore(checked, &item_text(dialog, IDC_IGNORE))),
        repository: item_text(dialog, IDC_REPOSITORY),
        module_limit: item_text(dialog, IDC_MODULE_LIMIT),
        byte_limit: item_text(dialog, IDC_BYTE_LIMIT),
    };
    let edit = match parse_form(&form) {
        Ok(edit) => edit,
        Err(message) => {
            message_box(dialog, &message, MB_ICONERROR);
            return false;
        }
    };
    match save_config_edit(&edit) {
        Ok((path, backup)) => {
            let mut message = format!(
                "保存しました: {}\r\n変更は AviUtl2 の再起動後に反映されます。",
                path.display()
            );
            if let Some(backup) = backup {
                message.push_str(&format!(
                    "\r\n元のファイルは解析できなかったため {} に退避しました。",
                    backup.display()
                ));
            }
            message_box(dialog, &message, MB_ICONINFORMATION);
            true
        }
        Err(message) => {
            message_box(dialog, &message, MB_ICONERROR);
            false
        }
    }
}

/// A `LVIS_STATEIMAGEMASK` state image: 1 = unchecked box, 2 = checked box.
fn state_image(checked: bool) -> u32 {
    ((if checked { 2 } else { 1 }) as u32) << 12
}

/// Populates the ignore checklist: one single-column row per known effect,
/// checkbox reflecting the current ignore list.
fn fill_ignore_list(dialog: WsHWND, rows: &[IgnoreRow]) {
    // SAFETY: `dialog` is the live dialog; every struct passed to SendMessageW
    // is initialized and alive for that call, and pszText buffers are
    // null-terminated.
    unsafe {
        let list = GetDlgItem(dialog, IDC_IGNORE_LIST as i32);
        if list.is_null() {
            return;
        }
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            LVS_EX_CHECKBOXES as WPARAM,
            LVS_EX_CHECKBOXES as LPARAM,
        );
        // One full-width column; the view is LVS_REPORT with no header. The
        // client rect is measured before any item exists, so leave room for
        // the vertical scrollbar the (usually long) list will show — sizing to
        // the bare width adds a useless horizontal scrollbar the moment the
        // vertical one appears.
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetClientRect(list, &mut client);
        let mut column: LVCOLUMNW = std::mem::zeroed();
        column.mask = LVCF_WIDTH;
        column.cx = client.right - client.left - GetSystemMetrics(SM_CXVSCROLL);
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            0,
            &column as *const LVCOLUMNW as LPARAM,
        );
        for (index, row) in rows.iter().enumerate() {
            let mut name: Vec<u16> = row.name.encode_utf16().chain([0]).collect();
            let mut item: LVITEMW = std::mem::zeroed();
            item.mask = LVIF_TEXT;
            item.iItem = index as i32;
            item.pszText = name.as_mut_ptr();
            let at = SendMessageW(list, LVM_INSERTITEMW, 0, &item as *const LVITEMW as LPARAM);
            // A failed insert would shift every later row off its index — the
            // pairing the save reads check states back through — and an
            // off-by-one read turns into a silently un-ignored effect. Stop
            // filling instead: the save refuses a list whose count does not
            // match its rows. Not expected outside comctl32 allocation
            // failure.
            if at != index as isize {
                log_warn(&format!(
                    "the ignore checklist could not be filled (insert of \"{}\" returned {at}); \
                     saving from this dialog is disabled",
                    row.name
                ));
                break;
            }
            let mut state: LVITEMW = std::mem::zeroed();
            state.state = state_image(row.ignored);
            state.stateMask = LVIS_STATEIMAGEMASK;
            SendMessageW(
                list,
                LVM_SETITEMSTATE,
                index as WPARAM,
                &state as *const LVITEMW as LPARAM,
            );
        }
    }
}

/// How many rows the checklist actually holds, or `None` without the control.
fn ignore_list_count(dialog: WsHWND) -> Option<usize> {
    // SAFETY: `dialog` is the live dialog; the message carries no pointers.
    unsafe {
        let list = GetDlgItem(dialog, IDC_IGNORE_LIST as i32);
        (!list.is_null()).then(|| SendMessageW(list, LVM_GETITEMCOUNT, 0, 0).max(0) as usize)
    }
}

/// Whether the checklist row at `index` is checked. A missing control reads
/// as unchecked, which the caller's empty `rows` makes unreachable anyway.
fn ignore_list_checked(dialog: WsHWND, index: usize) -> bool {
    // SAFETY: `dialog` is the live dialog; the message carries no pointers.
    unsafe {
        let list = GetDlgItem(dialog, IDC_IGNORE_LIST as i32);
        if list.is_null() {
            return false;
        }
        let state = SendMessageW(
            list,
            LVM_GETITEMSTATE,
            index as WPARAM,
            LVIS_STATEIMAGEMASK as LPARAM,
        );
        state as u32 & LVIS_STATEIMAGEMASK == state_image(true)
    }
}

fn message_box(owner: WsHWND, text: &str, icon: u32) {
    let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let caption: Vec<u16> = "AEXCompat multi-filter".encode_utf16().chain([0]).collect();
    // SAFETY: both buffers are null-terminated and alive for the call.
    unsafe { MessageBoxW(owner, text.as_ptr(), caption.as_ptr(), MB_OK | icon) };
}

fn set_item_text(dialog: WsHWND, id: u16, text: &str) {
    let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: `dialog` is the live dialog; the buffer is null-terminated.
    unsafe { SetDlgItemTextW(dialog, id as i32, text.as_ptr()) };
}

/// The full text of one dialog control (`GetDlgItemTextW` needs the length
/// guessed up front; length-then-read sizes it exactly).
fn item_text(dialog: WsHWND, id: u16) -> String {
    // SAFETY: `dialog` is the live dialog; the buffer is sized to the reported
    // length plus the terminator, and `GetWindowTextW` bounds itself to it.
    unsafe {
        let control = GetDlgItem(dialog, id as i32);
        if control.is_null() {
            return String::new();
        }
        let length = GetWindowTextLengthW(control).max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let copied = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32).max(0);
        String::from_utf16_lossy(&buffer[..copied as usize])
    }
}

// --- In-memory dialog template ---------------------------------------------

// Style bits, spelled as the SDK spells them (the windows-sys constants for
// dialog/edit/button styles come in mixed signednesses; the template wants
// plain u32s).
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_BORDER: u32 = 0x0080_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_POPUP: u32 = 0x8000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const DS_SETFONT: u32 = 0x40;
const DS_MODALFRAME: u32 = 0x80;
const ES_MULTILINE: u32 = 0x0004;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_WANTRETURN: u32 = 0x1000;
const ES_NUMBER: u32 = 0x2000;
const SS_NOPREFIX: u32 = 0x0080;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const LVS_REPORT: u32 = 0x0001;
const LVS_SINGLESEL: u32 = 0x0004;
const LVS_NOCOLUMNHEADER: u32 = 0x4000;

const ATOM_BUTTON: u16 = 0x0080;
const ATOM_EDIT: u16 = 0x0081;
const ATOM_STATIC: u16 = 0x0082;

/// A dialog item's window class: one of the predefined class atoms, or a
/// registered class by name (the checklist's `SysListView32`).
enum ItemClass {
    Atom(u16),
    Named(&'static str),
}

/// Builds the `DLGTEMPLATE` stream. Returned as `Vec<u32>` because the
/// template must be DWORD-aligned and a `Vec<u16>`'s allocation only promises
/// two-byte alignment.
fn build_dialog_template() -> Vec<u32> {
    let mut words: Vec<u16> = Vec::new();
    let mut items: u16 = 0;

    // Header: style, exstyle, item count (patched below), x, y, cx, cy,
    // no menu, default dialog class, title, then DS_SETFONT's point size + face.
    push_u32(
        &mut words,
        DS_SETFONT | DS_MODALFRAME | WS_POPUP | WS_CAPTION | WS_SYSMENU,
    );
    push_u32(&mut words, 0);
    let count_at = words.len();
    words.push(0);
    for value in [0i16, 0, 340, 333] {
        words.push(value as u16);
    }
    words.push(0);
    words.push(0);
    push_wsz(&mut words, "AEXCompat multi-filter の設定");
    words.push(9);
    push_wsz(&mut words, "Yu Gothic UI");

    let multiline =
        ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | WS_VSCROLL | WS_BORDER | WS_TABSTOP;
    let single = ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP;
    let number = ES_NUMBER | single;

    let mut item = |style: u32, rect: [i16; 4], id: u16, class: ItemClass, text: &str| {
        push_item(&mut words, style, rect, id, class, text);
        items += 1;
    };

    item(
        SS_NOPREFIX,
        [7, 7, 326, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "スキャンフォルダ (1行に1つ / 空欄なら After Effects と MediaCore の既定フォルダ)",
    );
    item(
        multiline,
        [7, 17, 326, 38],
        IDC_DIRS,
        ItemClass::Atom(ATOM_EDIT),
        "",
    );

    item(
        SS_NOPREFIX,
        [7, 61, 326, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "依存 DLL フォルダ (1行に1つ / 空欄なら After Effects の既定)",
    );
    item(
        multiline,
        [7, 71, 326, 28],
        IDC_DEPENDENCY_DIRS,
        ItemClass::Atom(ATOM_EDIT),
        "",
    );

    item(
        SS_NOPREFIX,
        [7, 105, 326, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "無視するエフェクト (チェック = 無視 / 一覧はスキャン結果と discovery キャッシュ)",
    );
    item(
        LVS_REPORT | LVS_SINGLESEL | LVS_NOCOLUMNHEADER | WS_BORDER | WS_TABSTOP,
        [7, 115, 326, 48],
        IDC_IGNORE_LIST,
        ItemClass::Named("SysListView32"),
        "",
    );
    item(
        SS_NOPREFIX,
        [7, 169, 326, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "一覧に無いエフェクトを手動指定 (1行に1つ / .aex 拡張子は省略可)",
    );
    item(
        multiline,
        [7, 179, 326, 16],
        IDC_IGNORE,
        ItemClass::Atom(ATOM_EDIT),
        "",
    );

    item(
        SS_NOPREFIX,
        [7, 201, 326, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "worker リポジトリ (通常は空欄: プラグインと同じフォルダの worker を使用)",
    );
    item(
        single,
        [7, 211, 326, 13],
        IDC_REPOSITORY,
        ItemClass::Atom(ATOM_EDIT),
        "",
    );

    item(
        SS_NOPREFIX,
        [7, 230, 170, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "依存 DLL 数の上限 (空欄 = 無制限)",
    );
    item(
        number,
        [181, 228, 60, 13],
        IDC_MODULE_LIMIT,
        ItemClass::Atom(ATOM_EDIT),
        "",
    );
    item(
        SS_NOPREFIX,
        [7, 246, 170, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "依存 DLL 合計バイト数の上限 (空欄 = 無制限)",
    );
    item(
        number,
        [181, 244, 90, 13],
        IDC_BYTE_LIMIT,
        ItemClass::Atom(ATOM_EDIT),
        "",
    );

    // Tall enough for every env-override line at once (four are possible).
    item(
        SS_NOPREFIX,
        [7, 262, 326, 34],
        IDC_ENV_NOTE,
        ItemClass::Atom(ATOM_STATIC),
        "",
    );
    item(
        SS_NOPREFIX,
        [7, 300, 326, 8],
        0xFFFF,
        ItemClass::Atom(ATOM_STATIC),
        "変更は AviUtl2 の再起動後に反映されます。",
    );

    item(
        BS_DEFPUSHBUTTON | WS_TABSTOP,
        [222, 312, 50, 14],
        IDC_OK,
        ItemClass::Atom(ATOM_BUTTON),
        "OK",
    );
    item(
        WS_TABSTOP,
        [277, 312, 56, 14],
        IDC_CANCEL,
        ItemClass::Atom(ATOM_BUTTON),
        "キャンセル",
    );

    words[count_at] = items;

    // Re-home the u16 stream in a DWORD-aligned allocation.
    let mut aligned = vec![0u32; words.len().div_ceil(2)];
    // SAFETY: the destination holds `words.len()` u16s (rounded up to a whole
    // number of u32s) and the ranges do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(words.as_ptr(), aligned.as_mut_ptr().cast(), words.len());
    }
    aligned
}

fn push_u32(words: &mut Vec<u16>, value: u32) {
    words.push((value & 0xFFFF) as u16);
    words.push((value >> 16) as u16);
}

/// A null-terminated UTF-16 string, in place.
fn push_wsz(words: &mut Vec<u16>, text: &str) {
    words.extend(text.encode_utf16());
    words.push(0);
}

/// One `DLGITEMTEMPLATE` (DWORD-aligned): style, exstyle, rect, id, the window
/// class (atom or name), the title, and no creation data.
fn push_item(
    words: &mut Vec<u16>,
    style: u32,
    rect: [i16; 4],
    id: u16,
    class: ItemClass,
    text: &str,
) {
    if words.len() % 2 != 0 {
        words.push(0);
    }
    push_u32(words, style | WS_CHILD | WS_VISIBLE);
    push_u32(words, 0);
    for value in rect {
        words.push(value as u16);
    }
    words.push(id);
    match class {
        ItemClass::Atom(atom) => {
            words.push(0xFFFF);
            words.push(atom);
        }
        ItemClass::Named(name) => push_wsz(words, name),
    }
    push_wsz(words, text);
    words.push(0);
}
