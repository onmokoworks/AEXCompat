//! Allocation-free fixed-size register reads missing from unicorn-engine's safe API.

use unicorn_engine::{RegisterX86, Unicorn, uc_error, uc_reg_read};

/// Caller-owned storage with the alignment required by Unicorn's x86 backend.
#[repr(C, align(32))]
pub struct YmmValue {
    bytes: [u8; 32],
}

impl YmmValue {
    #[must_use]
    pub const fn zeroed() -> Self {
        Self { bytes: [0; 32] }
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub const fn as_mut_bytes(&mut self) -> &mut [u8; 32] {
        &mut self.bytes
    }
}

/// Reads a 256-bit x86 YMM register into a caller-owned buffer.
///
/// The register range check and [`YmmValue`] type ensure Unicorn cannot write
/// beyond or with insufficient alignment for `destination`.
pub fn read_ymm<D>(
    unicorn: &Unicorn<'_, D>,
    register: RegisterX86,
    destination: &mut YmmValue,
) -> Result<(), uc_error> {
    if !(RegisterX86::YMM0 as i32..=RegisterX86::YMM31 as i32).contains(&(register as i32)) {
        return Err(uc_error::ARG);
    }
    unsafe {
        uc_reg_read(
            unicorn.get_handle(),
            register.into(),
            destination.bytes.as_mut_ptr().cast(),
        )
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicorn_engine::{Arch, Mode};

    #[test]
    fn ymm_value_has_native_register_alignment() {
        assert_eq!(std::mem::align_of::<YmmValue>(), 32);
        let value = YmmValue::zeroed();
        assert_eq!((value.as_bytes().as_ptr() as usize) % 32, 0);
    }

    #[test]
    fn read_ymm_round_trips_and_rejects_other_register_widths() {
        let unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        let expected = std::array::from_fn(|index| index as u8);
        unicorn
            .reg_write_long(RegisterX86::YMM3, &expected)
            .unwrap();

        let mut actual = YmmValue::zeroed();
        read_ymm(&unicorn, RegisterX86::YMM3, &mut actual).unwrap();
        assert_eq!(actual.as_bytes(), &expected);
        assert_eq!(
            read_ymm(&unicorn, RegisterX86::XMM3, &mut actual),
            Err(uc_error::ARG)
        );
    }
}
