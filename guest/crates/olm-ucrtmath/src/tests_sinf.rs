// Frozen original-UCRT observations from OLM F27c, not host libm expectations.
// tools/ucrt_math_probe/rosetta/results-f27c/sinf-first.csv.gz SHA256 0392d26de3e5e4af6e807cb6a0a614472b2d3e75e171c26e0eca1b7ca4d2cb75
// tools/ucrt_math_probe/rosetta/results-f27c/paper-results.json SHA256 3c84ea10bfe8fa1ab701377eee46ec6bdae8668c258e29d5a51ab462395ece5f
#[test]
fn sinf_retained_ucrt_bits() {
    let rows: &[(i32, u32, u32, u32)] = &[
        (0, 0x4b000300, 0x0, 0xbf580631),
        (0, 0x4b00033f, 0x0, 0xbf6bf771),
        (0, 0x4b0003b3, 0x0, 0x3f4dc4b8),
        (0, 0x4b0004fb, 0x0, 0x3f4dc3bd),
        (0, 0x4b000556, 0x0, 0xbf5cbf84),
        (0, 0x4b000766, 0x0, 0xbf3c73e6),
        (0, 0x4b000828, 0x0, 0xbf7fd72a),
        (0, 0x4b000dc0, 0x0, 0xbf530fab),
        (0, 0x4b000e00, 0x0, 0xbf57fef4),
        (0, 0x4b000e89, 0x0, 0xbf49b0ae),
        (0, 0x4b00147e, 0x0, 0x3f49988b),
        (0, 0x4b0014c0, 0x0, 0xbf4555e5),
        (0, 0x4b001652, 0x0, 0xbf57f897),
        (0, 0x4b0017d1, 0x0, 0xbf7520f8),
        (0, 0x4b001851, 0x0, 0x3f5f117f),
        (0, 0x4b001947, 0x0, 0x3f674b53),
        (1, 0x4b000300, 0x0, 0xbf580632),
        (1, 0x4b00033f, 0x0, 0xbf6bf772),
        (1, 0x4b0003b3, 0x0, 0x3f4dc4b7),
        (1, 0x4b0004fb, 0x0, 0x3f4dc3be),
        (1, 0x4b000556, 0x0, 0xbf5cbf83),
        (1, 0x4b000766, 0x0, 0xbf3c73e5),
        (1, 0x4b000828, 0x0, 0xbf7fd72b),
        (1, 0x4b000dc0, 0x0, 0xbf530fac),
        (1, 0x4b000e00, 0x0, 0xbf57fef3),
        (1, 0x4b000e89, 0x0, 0xbf49b0af),
        (1, 0x4b00147e, 0x0, 0x3f49988c),
        (1, 0x4b0014c0, 0x0, 0xbf4555e4),
        (1, 0x4b001652, 0x0, 0xbf57f896),
        (1, 0x4b0017d1, 0x0, 0xbf7520f7),
        (1, 0x4b001851, 0x0, 0x3f5f117e),
        (1, 0x4b001947, 0x0, 0x3f674b54),
    ];
    for &(mode, a, b, expected) in rows {
        let _ = b;
        assert_eq!(
            unsafe { ucrt_sinf(f32::from_bits(a), mode) }.to_bits(),
            expected,
            "mode={mode} a={a:x} b={b:x}"
        );
    }
}
