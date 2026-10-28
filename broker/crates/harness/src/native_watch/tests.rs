use super::*;

fn config() -> Config {
    serde_json::from_str(r#"{"module":"probe.dll","watches":[{"id":"second","instruction_rva":16,"register":"rcx","dereference_offsets":[8,0],"size":4,"occurrence":2}]}"#).unwrap()
}
struct Image {
    data: BTreeMap<u64, Vec<u8>>,
    reads: Vec<(u64, usize)>,
}
impl Memory for Image {
    fn read_exact(&mut self, at: u64, bytes: &mut [u8]) -> bool {
        self.reads.push((at, bytes.len()));
        match self.data.get(&at) {
            Some(data) if data.len() == bytes.len() => {
                bytes.copy_from_slice(data);
                true
            }
            _ => false,
        }
    }
}
fn image() -> Image {
    Image {
        data: BTreeMap::from([
            (108, 200u64.to_le_bytes().to_vec()),
            (200, 300u64.to_le_bytes().to_vec()),
            (300, vec![0, 1, 128, 255]),
        ]),
        reads: Vec::new(),
    }
}
#[test]
fn nth_touch_orders_threads_and_reads_only_declared_ranges() {
    let mut o = Observer::new(config()).unwrap();
    let mut m = image();
    let regs = BTreeMap::from([(Register::Rcx, 100)]);
    o.hit(17, 1, &regs, &mut m).unwrap();
    o.hit(16, 7, &regs, &mut m).unwrap();
    assert!(m.reads.is_empty());
    assert!(!o.complete());
    o.hit(16, 9, &regs, &mut m).unwrap();
    assert_eq!(m.reads, vec![(108, 8), (200, 8), (300, 4)]);
    let w = &o.witnesses[0];
    assert_eq!((w.thread_id, w.sequence, w.occurrence), (9, 2, 2));
    assert_eq!(w.before.hex.as_deref(), Some("000180ff"));
    assert_eq!(w.before.pointer_chain, vec![200, 300]);
    o.hit(16, 7, &regs, &mut m).unwrap();
    assert_eq!(m.reads.len(), 3);
    assert!(o.captured());
}
#[test]
fn relocation_changes_address_not_rva() {
    let o = Observer::new(config()).unwrap();
    assert_eq!(o.addresses(0x1000, 0x100).unwrap(), vec![0x1010]);
    assert_eq!(o.addresses(0x7000, 0x100).unwrap(), vec![0x7010]);
    assert!(o.addresses(u64::MAX - 8, 100).is_err());
    assert!(o.addresses(1, 16).is_err());
}
#[test]
fn failures_are_explicit_and_never_publish_partial_bytes() {
    for (reg, status, reads) in [
        (None, "missing_register", 0),
        (Some(u64::MAX), "address_overflow", 0),
        (Some(5), "pointer_unreadable", 1),
    ] {
        let mut m = image();
        let s = capture(&config().watches[0], reg, &mut m);
        assert_eq!(s.status, status);
        assert!(s.hex.is_none());
        assert_eq!(m.reads.len(), reads);
    }
    let mut m = image();
    m.data.insert(300, vec![1, 2]);
    let s = capture(&config().watches[0], Some(100), &mut m);
    assert_eq!(s.status, "unreadable");
    assert!(s.hex.is_none());
}
#[test]
fn all_bounds_and_ambiguous_specs_are_rejected() {
    let mut bad = Vec::new();
    for size in [0, MAX_BYTES + 1] {
        let mut c = config();
        c.watches[0].size = size;
        bad.push(c);
    }
    let mut c = config();
    c.watches[0].occurrence = 0;
    bad.push(c);
    let mut c = config();
    c.watches[0].dereference_offsets = vec![0; 9];
    bad.push(c);
    let mut c = config();
    c.watches[0].dereference_offsets = vec![4097];
    bad.push(c);
    let mut c = config();
    c.module = "C:\\probe.dll".into();
    bad.push(c);
    let mut c = config();
    c.watches.push(c.watches[0].clone());
    bad.push(c);
    let mut c = config();
    c.watches.clear();
    bad.push(c);
    let mut c = config();
    c.watches = (0..5)
        .map(|i| {
            let mut w = config().watches[0].clone();
            w.id = i.to_string();
            w.instruction_rva = i;
            w
        })
        .collect();
    bad.push(c);
    let mut c = config();
    c.watches = (0..3)
        .map(|i| {
            let mut w = config().watches[0].clone();
            w.id = i.to_string();
            w.instruction_rva = i;
            w.size = MAX_BYTES;
            w
        })
        .collect();
    bad.push(c);
    for c in bad {
        assert!(Observer::new(c).is_err());
    }
    for field in [
        r#""absolute_address":4"#,
        r#""when":"return""#,
        r#""occurrence":1,"occurrence":2"#,
    ] {
        let j = format!(r#"{{"id":"a","instruction_rva":1,"register":"rcx","size":4,{field}}}"#);
        assert!(serde_json::from_str::<Watch>(&j).is_err());
    }
}
#[test]
fn per_site_counts_and_event_sequence_are_separate() {
    let mut c = config();
    let mut w = c.watches[0].clone();
    w.id = "first".into();
    w.instruction_rva = 32;
    w.occurrence = 1;
    c.watches.push(w);
    let mut o = Observer::new(c).unwrap();
    let mut m = image();
    let r = BTreeMap::from([(Register::Rcx, 100)]);
    for rva in [16, 32, 16] {
        o.hit(rva, 1, &r, &mut m).unwrap();
    }
    assert_eq!(o.hits, vec![2, 1]);
    assert_eq!(
        o.witnesses.iter().map(|w| w.sequence).collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert!(o.captured());
    let value = serde_json::to_value(&o.witnesses).unwrap();
    assert_eq!(value[1]["before"]["status"], "captured");
    assert_eq!(value[1]["instruction_rva"], 16);
}
#[test]
fn final_range_overflow_is_checked_without_reading() {
    let mut w = config().watches.remove(0);
    w.dereference_offsets.clear();
    let mut m = image();
    let s = capture(&w, Some(u64::MAX - 2), &mut m);
    assert_eq!(s.status, "address_overflow");
    assert!(m.reads.is_empty());
}

#[test]
fn plane_capture_exceeds_small_cli_watch_size_without_exceeding_declared_read() {
    let mut c = config();
    let w = &mut c.watches[0];
    w.size = 8192;
    w.dereference_offsets.clear();
    w.occurrence = 1;
    let mut o = Observer::new(c).unwrap();
    let mut m = Image {
        data: BTreeMap::from([(100, vec![0xab; 8192])]),
        reads: Vec::new(),
    };
    o.hit(16, 1, &BTreeMap::from([(Register::Rcx, 100)]), &mut m)
        .unwrap();
    assert!(o.captured());
    assert_eq!(m.reads, vec![(100, 8192)]);
    assert_eq!(
        o.witnesses[0].before.hex.as_deref(),
        Some("ab".repeat(8192).as_str())
    );
}
