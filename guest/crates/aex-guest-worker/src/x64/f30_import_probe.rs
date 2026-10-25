/// Reproducible raw-bit diagnostic through the same legacy import hooks.
#[doc(hidden)]
pub fn f30_import_probe(name: &str, library: &str, count: u64) {
    use std::io::Write;
    const CODE: u64 = 0x1000_0000;
    let mut uc = Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
    uc.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
    uc.mem_write(CODE, &[0x90]).unwrap();
    match (name, dispatch_win64_import(library, name)) {
        ("sin", Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Sin)) => install_double_import(&mut uc, CODE, "sin", olm_ucrtmath::sin).unwrap(),
        ("cos", Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Cos)) => install_double_import(&mut uc, CODE, "cos", olm_ucrtmath::cos).unwrap(),
        ("exp", Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Exp)) => install_double_import(&mut uc, CODE, "exp", olm_ucrtmath::exp).unwrap(),
        ("pow", Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Pow)) => install_double_binary_import(&mut uc, CODE, "pow", olm_ucrtmath::pow).unwrap(),
        _ => panic!("wrong import dispatch"),
    }
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for i in 0..count {
        fn mix(mut z: u64) -> u64 {
            z = z.wrapping_add(0x9e3779b97f4a7c15);
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            z ^ (z >> 31)
        }
        let (x,y) = (mix(i ^ 0xf46), mix(i ^ 0x706f77));
        // Plugin ranges: normalized/HDR bases, sRGB gamma and repeat exponents.
        let (a,b) = if std::env::var("F46_RANGE").as_deref() == Ok("plugin") {
            let base = (x >> 11) as f64 / 9007199254740992.0;
            let exponent = match i % 4 { 0 => 2.4, 1 => 1.0/2.4,
                2 => (y % 100) as f64, _ => (y >> 11) as f64 / 9007199254740992.0 * 10.0 };
            ((if i % 8 == 0 {base * 16.0} else {base}).to_bits(), exponent.to_bits())
        } else { (x,y) };
        uc.reg_write(RegisterX86::XMM0, a as u64).unwrap();
        uc.reg_write(RegisterX86::XMM1, b as u64).unwrap();
        uc.emu_start(CODE, CODE+1, 0, 1).unwrap();
        let result = uc.reg_read(RegisterX86::XMM0).unwrap();
        for word in [a,b,result,result] { out.write_all(&word.to_le_bytes()).unwrap(); }
    }
    out.flush().unwrap();
}
