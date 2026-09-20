//! Allocation-free fixed-size register reads missing from unicorn-engine's safe API.

use unicorn_engine::{Prot, RegisterX86, Unicorn, uc_error, uc_reg_read};

/// Installs exact x86 instruction addresses where the vendored translator must
/// apply VEX.128 upper-lane semantics before executing the instruction.
pub fn set_x86_avx_sync_points<D>(
    unicorn: &Unicorn<'_, D>,
    addresses: &[u64],
    actions: &[u8],
) -> Result<(), uc_error> {
    if addresses.len() != actions.len()
        || actions.iter().any(|action| !(1..=18).contains(action))
        || addresses.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(uc_error::ARG);
    }
    unsafe {
        unicorn_engine::unicorn_const::uc_x86_set_avx_sync_points(
            unicorn.get_handle(),
            addresses.as_ptr(),
            actions.as_ptr(),
            addresses.len(),
        )
    }
    .into()
}

pub fn reset_x86_avx_defined_mask<D>(unicorn: &Unicorn<'_, D>) -> Result<(), uc_error> {
    unsafe { unicorn_engine::unicorn_const::uc_x86_reset_avx_defined_mask(unicorn.get_handle()) }
        .into()
}

/// Checks a mapped guest range without allocating Unicorn's complete region
/// snapshot.
pub fn range_has_protection<D>(
    unicorn: &Unicorn<'_, D>,
    address: u64,
    size: u64,
    protection: Prot,
) -> Result<bool, uc_error> {
    let mut allowed = false;
    unsafe {
        unicorn_engine::uc_mem_range_has_prot(
            unicorn.get_handle(),
            address,
            size,
            protection.0 as u32,
            &raw mut allowed,
        )
    }
    .and(Ok(allowed))
}

/// Enables or disables translated exit polling after every guest memory access.
///
/// Disabling is valid only for engines without memory hooks. Unicorn still
/// reports unmapped/protected accesses, and code hooks retain their own exit
/// checks. The vendored API rejects both disabling with existing memory hooks
/// and adding a memory hook while disabled.
pub fn set_memory_exit_checks<D>(unicorn: &Unicorn<'_, D>, enabled: bool) -> Result<(), uc_error> {
    unsafe { unicorn_engine::uc_set_memory_exit_checks(unicorn.get_handle(), enabled) }.into()
}

#[must_use]
pub fn x86_avx_defined_mask<D>(unicorn: &Unicorn<'_, D>) -> u32 {
    unsafe { unicorn_engine::unicorn_const::uc_x86_get_avx_defined_mask(unicorn.get_handle()) }
}

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
    use unicorn_engine::{Arch, HookType, Mode, TlbEntry, TlbType};

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

    #[test]
    fn range_protection_spans_adjacent_regions_without_snapshot_allocation() {
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(0x1000, 4096, Prot::READ).unwrap();
        unicorn.mem_map(0x2000, 4096, Prot::READ).unwrap();
        unicorn.mem_map(0x3000, 4096, Prot::WRITE).unwrap();

        assert!(range_has_protection(&unicorn, 0x1800, 4096, Prot::READ).unwrap());
        assert!(!range_has_protection(&unicorn, 0x2800, 4096, Prot::READ).unwrap());
        assert!(!range_has_protection(&unicorn, u64::MAX, 2, Prot::READ).unwrap());
        assert!(range_has_protection(&unicorn, u64::MAX, 0, Prot::READ).unwrap());
    }

    #[test]
    fn memory_exit_fast_path_keeps_faults_and_code_hook_stops_fail_closed() {
        const CODE: u64 = 0x1000;
        const DATA: u64 = 0x2000;
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(CODE, 4096, Prot::ALL).unwrap();
        unicorn
            .mem_map(DATA, 4096, Prot::READ | Prot::WRITE)
            .unwrap();
        // inc byte [rax]; inc byte [rax]
        unicorn.mem_write(CODE, &[0xfe, 0x00, 0xfe, 0x00]).unwrap();
        set_memory_exit_checks(&unicorn, false).unwrap();
        assert_eq!(
            unicorn.add_mem_hook(HookType::MEM_WRITE, 1, 0, |_, _, _, _, _| true),
            Err(uc_error::ARG),
        );
        assert_eq!(
            unicorn.add_tlb_hook(1, 0, |_, address, _| Some(TlbEntry {
                paddr: address,
                perms: Prot::ALL,
            })),
            Err(uc_error::ARG),
        );
        unicorn
            .add_code_hook(CODE + 2, CODE + 2, |unicorn, _, _| {
                unicorn.emu_stop().unwrap();
            })
            .unwrap();
        unicorn.reg_write(RegisterX86::RAX, DATA).unwrap();
        unicorn.emu_start(CODE, CODE + 4, 0, 0).unwrap();
        assert_eq!(unicorn.mem_read_as_vec(DATA, 1).unwrap(), [1]);

        let mut faulting = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        faulting.mem_map(CODE, 4096, Prot::ALL).unwrap();
        faulting.mem_write(CODE, &[0xc6, 0x00, 7]).unwrap();
        set_memory_exit_checks(&faulting, false).unwrap();
        faulting.reg_write(RegisterX86::RAX, DATA).unwrap();
        assert_eq!(
            faulting.emu_start(CODE, CODE + 3, 0, 0),
            Err(uc_error::WRITE_UNMAPPED),
        );
    }

    #[test]
    fn memory_exit_fast_path_rejects_existing_memory_hooks() {
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn
            .add_mem_hook(HookType::MEM_WRITE, 1, 0, |_, _, _, _, _| true)
            .unwrap();
        assert_eq!(set_memory_exit_checks(&unicorn, false), Err(uc_error::ARG));

        let mut tlb = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        tlb.ctl_set_tlb_type(TlbType::VIRTUAL).unwrap();
        const CODE: u64 = 0x1000;
        const PHYSICAL_DATA: u64 = 0x2000;
        const VIRTUAL_DATA: u64 = 0x3000;
        tlb.mem_map(CODE, 4096, Prot::ALL).unwrap();
        tlb.mem_map(PHYSICAL_DATA, 4096, Prot::READ | Prot::WRITE)
            .unwrap();
        // inc byte [rax]; inc byte [rax]
        tlb.mem_write(CODE, &[0xfe, 0x00, 0xfe, 0x00]).unwrap();
        tlb.add_tlb_hook(1, 0, |unicorn, address, _| {
            if address == VIRTUAL_DATA {
                unicorn.emu_stop().unwrap();
            }
            Some(TlbEntry {
                paddr: if address == VIRTUAL_DATA {
                    PHYSICAL_DATA
                } else {
                    address
                },
                perms: Prot::ALL,
            })
        })
        .unwrap();
        assert_eq!(set_memory_exit_checks(&tlb, false), Err(uc_error::ARG));
        tlb.reg_write(RegisterX86::RAX, VIRTUAL_DATA).unwrap();
        tlb.emu_start(CODE, CODE + 4, 0, 0).unwrap();
        assert_eq!(tlb.mem_read_as_vec(PHYSICAL_DATA, 1).unwrap(), [0]);
    }

    #[test]
    fn fragmented_ram_remaps_preserve_contents_and_page_permissions() {
        use unicorn_engine::Prot;
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(0x1000, 4096, Prot::ALL).unwrap();
        unicorn.mem_write(0x1000, &[0xc6, 0x00, 7]).unwrap(); // mov byte [rax],7
        unicorn
            .mem_protect(0x1000, 4096, Prot::READ | Prot::EXEC)
            .unwrap();
        for index in 0..128u64 {
            let address = 0x100000 + index * 4096;
            unicorn
                .mem_map(address, 4096, Prot::READ | Prot::WRITE)
                .unwrap();
            unicorn.mem_write(address, &index.to_le_bytes()).unwrap();
        }
        for index in (0..128u64).step_by(2) {
            unicorn.mem_unmap(0x100000 + index * 4096, 4096).unwrap();
        }
        for index in (1..128u64).step_by(2) {
            unicorn
                .mem_protect(0x100000 + index * 4096, 4096, Prot::READ)
                .unwrap();
        }
        unicorn.reg_write(RegisterX86::RAX, 0x100000).unwrap();
        assert_eq!(
            unicorn.emu_start(0x1000, 0x1003, 1000000, 1),
            Err(uc_error::WRITE_UNMAPPED)
        );
        unicorn.reg_write(RegisterX86::RAX, 0x101000).unwrap();
        assert_eq!(
            unicorn.emu_start(0x1000, 0x1003, 1000000, 1),
            Err(uc_error::WRITE_PROT)
        );
        for index in (0..128u64).step_by(2) {
            let address = 0x100000 + index * 4096;
            unicorn
                .mem_map(address, 4096, Prot::READ | Prot::WRITE)
                .unwrap();
            unicorn.reg_write(RegisterX86::RAX, address).unwrap();
            unicorn.emu_start(0x1000, 0x1003, 1000000, 1).unwrap();
            assert_eq!(unicorn.mem_read_as_vec(address, 1).unwrap(), [7]);
        }
        for index in (1..128u64).step_by(2) {
            assert_eq!(
                unicorn.mem_read_as_vec(0x100000 + index * 4096, 8).unwrap(),
                index.to_le_bytes()
            );
        }
    }
    #[test]
    #[ignore = "manual memory mapping scalability measurement"]
    fn benchmark_fragmented_mapping_updates() {
        use unicorn_engine::Prot;
        for count in [128u64, 1024, 4096] {
            let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
            let started = std::time::Instant::now();
            for index in 0..count {
                uc.mem_map(0x100000 + index * 8192, 4096, Prot::READ | Prot::WRITE)
                    .unwrap();
            }
            let map_time = started.elapsed();
            let started = std::time::Instant::now();
            for index in 0..count {
                uc.mem_unmap(0x100000 + index * 8192, 4096).unwrap();
            }
            assert!(uc.mem_regions().unwrap().is_empty());
            eprintln!(
                "fragmented_mapping count={count} map={map_time:?} unmap={:?}",
                started.elapsed()
            );
        }
    }
    #[test]
    #[ignore = "manual allocation after free measurement"]
    fn benchmark_allocation_after_free() {
        use unicorn_engine::Prot;
        for count in [128u64, 1024, 4096] {
            let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
            for index in 0..count {
                uc.mem_map(0x100000 + index * 8192, 4096, Prot::ALL)
                    .unwrap();
            }
            let started = std::time::Instant::now();
            for index in 0..128u64 {
                let address = 0x100000 + (index * 137 % count) * 8192;
                uc.mem_unmap(address, 4096).unwrap();
                uc.mem_map(address, 4096, Prot::ALL).unwrap();
                uc.mem_write(address, &index.to_le_bytes()).unwrap();
                assert_eq!(uc.mem_read_as_vec(address, 8).unwrap(), index.to_le_bytes());
            }
            eprintln!(
                "allocation_after_free count={count} cycles=128 elapsed={:?}",
                started.elapsed()
            );
        }
    }
    #[test]
    fn many_shuffled_regions_preserve_address_order_and_contents() {
        use unicorn_engine::Prot;
        let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        let address = |index: u64| 0x100000 + ((index % 8) << 40) + (index / 8) * 8192;
        for step in 0..320u64 {
            let index = (step * 137) % 320;
            uc.mem_map(address(index), 4096, Prot::READ | Prot::WRITE)
                .unwrap();
            uc.mem_write(address(index), &index.to_le_bytes()).unwrap();
        }
        for index in 0..320u64 {
            assert_eq!(
                uc.mem_read_as_vec(address(index), 8).unwrap(),
                index.to_le_bytes()
            );
        }
        assert_eq!(
            uc.mem_map(address(100), 4096, Prot::READ),
            Err(uc_error::MAP)
        );
        for index in (0..320u64).step_by(3) {
            uc.mem_unmap(address(index), 4096).unwrap();
            assert!(uc.mem_read_as_vec(address(index), 1).is_err());
        }
        for index in 0..320u64 {
            if index % 3 != 0 {
                assert_eq!(
                    uc.mem_read_as_vec(address(index), 8).unwrap(),
                    index.to_le_bytes()
                );
            }
        }
    }
}
