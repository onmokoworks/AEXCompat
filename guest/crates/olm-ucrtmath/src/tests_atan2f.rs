// Frozen original-UCRT observations from OLM F27c, not host libm expectations.
// tools/ucrt_math_probe/rosetta/results-f27c/atan2f-first.csv.gz SHA256 6280f1b25a26580475f5b5fbc3caee10be7348acbbfdc8a4aa9903fdc03500f4
// tools/ucrt_math_probe/rosetta/results-f27c/paper-results.json SHA256 3c84ea10bfe8fa1ab701377eee46ec6bdae8668c258e29d5a51ab462395ece5f
#[test]
fn atan2f_retained_ucrt_bits() {
    let rows: &[(i32, u32, u32, u32)] = &[
        (0, 0x1b838e5, 0xf5b26167, 0x40490fdb),
        (0, 0x385cf00, 0x9867b125, 0x40490fdb),
        (0, 0x5c93453, 0xed9b6898, 0x40490fdb),
        (0, 0x9db1a15, 0xb82f1e01, 0x40490fdb),
        (0, 0xb11d2a3, 0xc63153eb, 0x40490fdb),
        (0, 0xfcebf0e, 0xa3b93163, 0x40490fdb),
        (0, 0x139cf752, 0xd8f16e67, 0x40490fdb),
        (0, 0x14ebae67, 0xefea0bca, 0x40490fdb),
        (0, 0x15100c57, 0xfdff499c, 0x40490fdb),
        (0, 0x1c88de9d, 0xf5362696, 0x40490fdb),
        (0, 0x1fe1787f, 0xc68a12d2, 0x40490fdb),
        (0, 0x204b00bb, 0x67fc4680, 0x34),
        (0, 0x24ac9ffe, 0xf72f759f, 0x40490fdb),
        (0, 0x28f0546d, 0xe1d61000, 0x40490fdb),
        (0, 0x338a6fe8, 0xc4b54997, 0x40490fdb),
        (0, 0x373ce89c, 0xfb87872b, 0x40490fdb),
        (0, 0x473a6ceb, 0xf29903e3, 0x40490fdb),
        (0, 0x4a6ef1f7, 0xdc08ea68, 0x40490fdb),
        (0, 0x5d7fff12, 0x6400056e, 0x38fff437),
        (0, 0x7d508082, 0x7e4cf8c1, 0x3e7f00f4),
        (0, 0x81e65778, 0xd5869e34, 0xc0490fdb),
        (0, 0x82a52681, 0xc9e41aab, 0xc0490fdb),
        (0, 0x83f6d1ff, 0x4f1dfd4, 0xbe7fc772),
        (0, 0x8462b82c, 0xd6635892, 0xc0490fdb),
        (0, 0x84ac53f6, 0x22c53df, 0xbfc51030),
        (0, 0x85e73167, 0x765fcd8, 0xbdfffffa),
        (0, 0x86de1b92, 0xdfbb3537, 0xc0490fdb),
        (0, 0x87b86ea1, 0xb3b4c09f, 0xc0490fdb),
        (0, 0x88b5b9a6, 0xd638bfa1, 0xc0490fdb),
        (0, 0x8bc4aee7, 0xa35862bc, 0xc0490fdb),
        (0, 0x8d4df415, 0xd4967d9, 0xbf4beb79),
        (0, 0x8ef97c8b, 0xa03d53e8, 0xc0490fdb),
        (0, 0xa02755b5, 0xde45263a, 0xc0490fdb),
        (0, 0xa6c64550, 0xe43f1263, 0xc0490fdb),
        (0, 0xa99d0eb4, 0xe06a8fc4, 0xc0490fdb),
        (0, 0xad7b56e2, 0xc701a97f, 0xc0490fdb),
        (0, 0xae10d226, 0xdcfbaa05, 0xc0490fdb),
        (0, 0xb07c8506, 0xf395b9b5, 0xc0490fdb),
        (0, 0xb779b13b, 0xf7e2984f, 0xc0490fdb),
        (0, 0xd5807b79, 0x56fff49a, 0xbdffac3b),
        (0, 0xd87ce7a4, 0x597b02cc, 0xbe7cace0),
        (1, 0x204b00bb, 0x67fc4680, 0x34),
        (1, 0x5d7fff12, 0x6400056e, 0x38fff437),
        (1, 0x7d508082, 0x7e4cf8c1, 0x3e7f00f4),
        (1, 0x83f6d1ff, 0x4f1dfd4, 0xbe7fc772),
        (1, 0x84ac53f6, 0x22c53df, 0xbfc51030),
        (1, 0x85e73167, 0x765fcd8, 0xbdfffffa),
        (1, 0x8d4df415, 0xd4967d9, 0xbf4beb79),
        (1, 0xd5807b79, 0x56fff49a, 0xbdffac3b),
        (1, 0xd87ce7a4, 0x597b02cc, 0xbe7cace0),
    ];
    for &(mode, a, b, expected) in rows {
        let _ = b;
        assert_eq!(
            unsafe { ucrt_atan2f(f32::from_bits(a), f32::from_bits(b), mode) }.to_bits(),
            expected,
            "mode={mode} a={a:x} b={b:x}"
        );
    }
}
