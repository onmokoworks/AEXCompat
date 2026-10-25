// Frozen original-UCRT observations from OLM F27c, not host libm expectations.
// tools/ucrt_math_probe/rosetta/results-f27c/cosf-first.csv.gz SHA256 338613f18759de09666e4900c36e74e64b37aa006885fd451936b973f05313ba
// tools/ucrt_math_probe/rosetta/results-f27c/paper-results.json SHA256 3c84ea10bfe8fa1ab701377eee46ec6bdae8668c258e29d5a51ab462395ece5f
#[test]
fn cosf_retained_ucrt_bits() {
    let rows: &[(i32, u32, u32, u32)] = &[
        (0, 0x3a544395, 0x0, 0x3f7ffffb),
        (0, 0x3a77def6, 0x0, 0x3f7ffff9),
        (0, 0x3a8b7c1a, 0x0, 0x3f7ffff7),
        (0, 0x3a92a476, 0x0, 0x3f7ffff6),
        (0, 0x3a997774, 0x0, 0x3f7ffff5),
        (0, 0x3aa646e2, 0x0, 0x3f7ffff3),
        (0, 0x3ab22b21, 0x0, 0x3f7ffff1),
        (0, 0x3ab7d376, 0x0, 0x3f7ffff0),
        (0, 0x3abd5087, 0x0, 0x3f7fffef),
        (0, 0x3ac2a5fe, 0x0, 0x3f7fffee),
        (0, 0x3ac7d707, 0x0, 0x3f7fffed),
        (0, 0x3acce665, 0x0, 0x3f7fffec),
        (0, 0x3acce666, 0x0, 0x3f7fffec),
        (0, 0x3ad1d68a, 0x0, 0x3f7fffeb),
        (0, 0x3ad6a99c, 0x0, 0x3f7fffea),
        (0, 0x3adb6186, 0x0, 0x3f7fffe9),
        (1, 0x3a544395, 0x0, 0x3f7ffffa),
        (1, 0x3a77def6, 0x0, 0x3f7ffff8),
        (1, 0x3a8b7c1a, 0x0, 0x3f7ffff6),
        (1, 0x3a92a476, 0x0, 0x3f7ffff5),
        (1, 0x3a997774, 0x0, 0x3f7ffff4),
        (1, 0x3aa646e2, 0x0, 0x3f7ffff2),
        (1, 0x3ab22b21, 0x0, 0x3f7ffff0),
        (1, 0x3ab7d376, 0x0, 0x3f7fffef),
        (1, 0x3abd5087, 0x0, 0x3f7fffee),
        (1, 0x3ac2a5fe, 0x0, 0x3f7fffed),
        (1, 0x3ac7d707, 0x0, 0x3f7fffec),
        (1, 0x3acce665, 0x0, 0x3f7fffeb),
        (1, 0x3acce666, 0x0, 0x3f7fffeb),
        (1, 0x3ad1d68a, 0x0, 0x3f7fffea),
        (1, 0x3ad6a99c, 0x0, 0x3f7fffe9),
        (1, 0x3adb6186, 0x0, 0x3f7fffe8),
    ];
    for &(mode, a, b, expected) in rows {
        let _ = b;
        assert_eq!(
            unsafe { ucrt_cosf(f32::from_bits(a), mode) }.to_bits(),
            expected,
            "mode={mode} a={a:x} b={b:x}"
        );
    }
}
