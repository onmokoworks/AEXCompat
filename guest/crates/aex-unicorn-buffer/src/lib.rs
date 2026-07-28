//! Allocation-free fixed-size register reads missing from unicorn-engine's safe API.

use unicorn_engine::{RegisterX86, Unicorn, uc_error, uc_reg_read};

/// Reads a 256-bit x86 YMM register into a caller-owned buffer.
///
/// The register range check ensures Unicorn cannot write more than the 32 bytes
/// supplied by `destination`.
pub fn read_ymm<D>(
    unicorn: &Unicorn<'_, D>,
    register: RegisterX86,
    destination: &mut [u8; 32],
) -> Result<(), uc_error> {
    if !(RegisterX86::YMM0 as i32..=RegisterX86::YMM31 as i32).contains(&(register as i32)) {
        return Err(uc_error::ARG);
    }
    unsafe {
        uc_reg_read(
            unicorn.get_handle(),
            register.into(),
            destination.as_mut_ptr().cast(),
        )
    }
    .into()
}
