/// Reproducible raw-bit diagnostic through the same legacy import hooks.
#[doc(hidden)]
pub fn f30_import_probe(name: &str, library: &str, count: u64) {
    use std::io::Write;
    const CODE: u64 = 0x1000_0000;
    let mut uc = Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
    uc.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
    uc.mem_write(CODE, &[0x90]).unwrap();
    match (name, dispatch_win64_import(library, name)) {
        ("expf", Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::ExpF)) => install_float_import(&mut uc, CODE, "expf", olm_ucrtmath::expf).unwrap(),
        ("powf", Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::PowF)) => install_float_binary_import(&mut uc, CODE, "powf", olm_ucrtmath::powf).unwrap(),
        ("atan2f", Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Atan2F)) => install_atan2f_import(&mut uc, CODE).unwrap(),
        _ => panic!("wrong import dispatch"),
    }
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for i in 0..count {
        let mut z = (i ^ 0xf30).wrapping_add(0x9e3779b97f4a7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        let (a,b) = (z as u32, (z >> 32) as u32);
        uc.reg_write(RegisterX86::XMM0, a as u64).unwrap();
        uc.reg_write(RegisterX86::XMM1, b as u64).unwrap();
        uc.emu_start(CODE, CODE+1, 0, 1).unwrap();
        let result = uc.reg_read(RegisterX86::XMM0).unwrap() as u32;
        for word in [a,b,result,result] { out.write_all(&word.to_le_bytes()).unwrap(); }
    }
    out.flush().unwrap();
}
