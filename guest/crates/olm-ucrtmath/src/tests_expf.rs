// Frozen original-UCRT observations from OLM F27c, not host libm expectations.
// tools/ucrt_math_probe/rosetta/results-f27c/expf-first.csv.gz SHA256 2adf26482743c79db29430b82d3e5e8409186d54d36fcd8c642d5b8bd6d8be66
// tools/ucrt_math_probe/rosetta/results-f27c/paper-results.json SHA256 3c84ea10bfe8fa1ab701377eee46ec6bdae8668c258e29d5a51ab462395ece5f
#[test]
fn expf_retained_ucrt_bits() {
    let rows: &[(i32, u32, u32, u32)] = &[
        (0, 0x33800000, 0x0, 0x3f800001),
        (0, 0x343fffff, 0x0, 0x3f800002),
        (0, 0x349fffff, 0x0, 0x3f800003),
        (0, 0x34a00000, 0x0, 0x3f800003),
        (0, 0x34dffffd, 0x0, 0x3f800004),
        (0, 0x34dffffe, 0x0, 0x3f800004),
        (0, 0x34dfffff, 0x0, 0x3f800004),
        (0, 0x350ffffe, 0x0, 0x3f800005),
        (0, 0x350fffff, 0x0, 0x3f800005),
        (0, 0x35100000, 0x0, 0x3f800005),
        (0, 0x352ffffd, 0x0, 0x3f800006),
        (0, 0x352ffffe, 0x0, 0x3f800006),
        (0, 0x352fffff, 0x0, 0x3f800006),
        (0, 0x354ffffb, 0x0, 0x3f800007),
        (0, 0x354ffffc, 0x0, 0x3f800007),
        (0, 0x354ffffd, 0x0, 0x3f800007),
        (0, 0x354ffffe, 0x0, 0x3f800007),
        (0, 0x354fffff, 0x0, 0x3f800007),
        (0, 0x35500000, 0x0, 0x3f800007),
        (0, 0x356ffff9, 0x0, 0x3f800008),
        (0, 0x356ffffa, 0x0, 0x3f800008),
        (0, 0x356ffffb, 0x0, 0x3f800008),
        (0, 0x356ffffc, 0x0, 0x3f800008),
        (0, 0x356ffffd, 0x0, 0x3f800008),
        (0, 0x356ffffe, 0x0, 0x3f800008),
        (0, 0x356fffff, 0x0, 0x3f800008),
        (0, 0x3587fffc, 0x0, 0x3f800009),
        (0, 0x3587fffd, 0x0, 0x3f800009),
        (0, 0x3587fffe, 0x0, 0x3f800009),
        (0, 0x3587ffff, 0x0, 0x3f800009),
        (0, 0xc2890f89, 0x0, 0xe0c39fc),
        (1, 0xc2890f89, 0x0, 0xe0c39fd),
    ];
    for &(mode, a, b, expected) in rows {
        let _ = b;
        assert_eq!(
            unsafe { ucrt_expf(f32::from_bits(a), mode) }.to_bits(),
            expected,
            "mode={mode} a={a:x} b={b:x}"
        );
    }
}
