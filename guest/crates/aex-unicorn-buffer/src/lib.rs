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

/// Reads only when the complete guest range is mapped and readable.
///
/// Unlike a separate [`range_has_protection`] and `Unicorn::mem_read` pair,
/// the common single-region path performs one mapping lookup. Mapping and
/// permission failures leave `destination` untouched.
pub fn read_protected<D>(
    unicorn: &Unicorn<'_, D>,
    address: u64,
    destination: &mut [u8],
) -> Result<(), uc_error> {
    unsafe {
        unicorn_engine::uc_mem_read_protected(
            unicorn.get_handle(),
            address,
            destination.as_mut_ptr().cast(),
            destination.len() as u64,
        )
    }
    .into()
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

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    unsafe extern "C" {
        fn pthread_jit_write_protect_np(enabled: i32);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn make_current_thread_jit_writable() {
        // SAFETY: This is the platform API Unicorn itself uses. The callback
        // guard must restore executable mode before translated code resumes.
        unsafe { pthread_jit_write_protect_np(0) };
    }

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
    fn x86_indirect_call_and_return_honor_nonzero_code_segment_base() {
        const CS: u64 = 0x100;
        const BASE: u64 = CS * 16;
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_16).unwrap();
        unicorn.mem_map(0, 0x4000, Prot::ALL).unwrap();

        unicorn.reg_write(RegisterX86::CS, CS).unwrap();
        unicorn.reg_write(RegisterX86::SP, 0x800).unwrap();

        let mut code = vec![0x90; 0x21];
        // mov ax, 0x10; call ax; mov bx, 0x1234; jmp 0x20
        code[0..10].copy_from_slice(&[0xb8, 0x10, 0x00, 0xff, 0xd0, 0xbb, 0x34, 0x12, 0xeb, 0x16]);
        // mov cx, 0x5678; ret
        code[0x10..0x14].copy_from_slice(&[0xb9, 0x78, 0x56, 0xc3]);
        unicorn.mem_write(BASE, &code).unwrap();

        for _ in 0..8 {
            unicorn.reg_write(RegisterX86::BX, 0).unwrap();
            unicorn.reg_write(RegisterX86::CX, 0).unwrap();
            unicorn.emu_start(BASE, BASE + 0x21, 0, 0).unwrap();
            assert_eq!(unicorn.reg_read(RegisterX86::BX).unwrap(), 0x1234);
            assert_eq!(unicorn.reg_read(RegisterX86::CX).unwrap(), 0x5678);
            assert_eq!(unicorn.reg_read(RegisterX86::SP).unwrap(), 0x800);
        }
    }

    #[test]
    fn x86_indirect_calls_preserve_registers_with_warm_and_colliding_targets() {
        for second in [0x5000u64, 0x0100_4000] {
            let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
            for base in [0x1000, 0x4000, second, 0x8000] {
                unicorn.mem_map(base, 4096, Prot::ALL).unwrap();
            }
            // The second case has identical jump-cache hash bits for both
            // callees. Full PC validation must reject the colliding entry.
            unicorn.mem_write(0x4000, &[0x83, 0xc3, 3, 0xc3]).unwrap();
            unicorn.mem_write(second, &[0x83, 0xc3, 7, 0xc3]).unwrap();
            // mov ecx,64; xor ebx,ebx; (mov rax,target; call rax) x2;
            // dec ecx; jnz to the first mov rax.
            let mut code = vec![0xb9, 64, 0, 0, 0, 0x31, 0xdb];
            for target in [0x4000u64, second] {
                code.extend_from_slice(&[0x48, 0xb8]);
                code.extend_from_slice(&target.to_le_bytes());
                code.extend_from_slice(&[0xff, 0xd0]);
            }
            code.extend_from_slice(&[0xff, 0xc9, 0x75, 0xe4]);
            unicorn.mem_write(0x1000, &code).unwrap();
            for _ in 0..8 {
                unicorn.reg_write(RegisterX86::RSP, 0x8800).unwrap();
                unicorn
                    .emu_start(0x1000, 0x1000 + code.len() as u64, 0, 0)
                    .unwrap();
                assert_eq!(unicorn.reg_read(RegisterX86::RBX).unwrap(), 640);
                assert_eq!(unicorn.reg_read(RegisterX86::RCX).unwrap(), 0);
                assert_eq!(unicorn.reg_read(RegisterX86::RSP).unwrap(), 0x8800);
            }
        }
    }

    #[test]
    fn x86_indirect_dispatch_preserves_flags_and_memory_in_32_and_64_bit_modes() {
        for mode in [Mode::MODE_32, Mode::MODE_64] {
            let mut unicorn = Unicorn::new(Arch::X86, mode).unwrap();
            unicorn.mem_map(0x1000, 0x8000, Prot::ALL).unwrap();
            // mov eax,0x4000; mov edx,0x7000; call eax/rax; nop
            let code = [0xb8, 0, 0x40, 0, 0, 0xba, 0, 0x70, 0, 0, 0xff, 0xd0, 0x90];
            unicorn.mem_write(0x1000, &code).unwrap();
            // adc dword ptr [edx/rdx],0; mov ebx,[edx/rdx]; ret
            unicorn.mem_write(0x4000, &[0x83, 0x12, 0, 0x8b, 0x1a, 0xc3]).unwrap();
            for carry in [0_u64, 1, 1, 0, 1, 0, 0, 1] {
                unicorn.mem_write(0x7000, &41_u32.to_le_bytes()).unwrap();
                unicorn.reg_write(RegisterX86::ESP, 0x8800).unwrap();
                unicorn.reg_write(RegisterX86::EFLAGS, 2 | carry).unwrap();
                unicorn.emu_start(0x1000, 0x100d, 0, 0).unwrap();
                assert_eq!(unicorn.reg_read(RegisterX86::EBX).unwrap(), 41 + carry);
                assert_eq!(unicorn.mem_read_as_vec(0x7000, 4).unwrap(),
                           (41_u32 + carry as u32).to_le_bytes());
                assert_eq!(unicorn.reg_read(RegisterX86::ESP).unwrap(), 0x8800);
                // ADC cleared carry; its lazy flags must also survive RET.
                assert_eq!(unicorn.reg_read(RegisterX86::EFLAGS).unwrap() & 1, 0);
            }
        }
    }

    fn indirect_call_engine(target: u64) -> Unicorn<'static, ()> {
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(0x1000, 4096, Prot::ALL).unwrap();
        unicorn.mem_map(target & !0xfff, 8192, Prot::ALL).unwrap();
        unicorn
            .mem_map(0x8000, 4096, Prot::READ | Prot::WRITE)
            .unwrap();
        let mut code = vec![0x48, 0xb8]; // mov rax,target; call rax; nop
        code.extend_from_slice(&target.to_le_bytes());
        code.extend_from_slice(&[0xff, 0xd0, 0x90]);
        unicorn.mem_write(0x1000, &code).unwrap();
        unicorn
    }

    fn run_indirect_call(unicorn: &mut Unicorn<'_, ()>) -> Result<u64, uc_error> {
        unicorn.reg_write(RegisterX86::RSP, 0x8800)?;
        unicorn.reg_write(RegisterX86::RBX, 0)?;
        unicorn.emu_start(0x1000, 0x100d, 0, 0)?;
        assert_eq!(unicorn.reg_read(RegisterX86::RSP)?, 0x8800);
        unicorn.reg_read(RegisterX86::RBX)
    }

    #[test]
    fn x86_indirect_cache_respects_second_page_code_invalidation_and_flush() {
        const TARGET: u64 = 0x4ffe;
        let mut unicorn = indirect_call_engine(TARGET);
        // mov ebx,0x11223344; ret -- the immediate crosses a page boundary.
        unicorn
            .mem_write(TARGET, &[0xbb, 0x44, 0x33, 0x22, 0x11, 0xc3])
            .unwrap();
        for _ in 0..8 {
            assert_eq!(run_indirect_call(&mut unicorn), Ok(0x11223344));
        }
        unicorn.mem_write(0x5000, &[0x66, 0x77, 0x88]).unwrap();
        unicorn.ctl_remove_cache(0x5000, 0x6000).unwrap();
        for _ in 0..8 {
            assert_eq!(run_indirect_call(&mut unicorn), Ok(0x88776644));
        }
        unicorn.ctl_flush_tb().unwrap();
        assert_eq!(run_indirect_call(&mut unicorn), Ok(0x88776644));
        unicorn.ctl_flush_tlb().unwrap();
        assert_eq!(run_indirect_call(&mut unicorn), Ok(0x88776644));
    }

    #[test]
    fn x86_indirect_cache_respects_remap_and_explicit_permission_invalidation() {
        let mut unicorn = indirect_call_engine(0x4000);
        unicorn
            .mem_write(0x4000, &[0xbb, 1, 0, 0, 0, 0xc3])
            .unwrap();
        for _ in 0..8 {
            assert_eq!(run_indirect_call(&mut unicorn), Ok(1));
        }
        unicorn.mem_unmap(0x4000, 8192).unwrap();
        unicorn.mem_map(0x4000, 8192, Prot::ALL).unwrap();
        unicorn
            .mem_write(0x4000, &[0xbb, 2, 0, 0, 0, 0xc3])
            .unwrap();
        for _ in 0..8 {
            assert_eq!(run_indirect_call(&mut unicorn), Ok(2));
        }
        unicorn
            .mem_protect(0x4000, 8192, Prot::READ | Prot::WRITE)
            .unwrap();
        // Warm TBs require explicit invalidation after host permission changes
        // in the baseline too; automatic invalidation is tracked separately.
        unicorn.ctl_remove_cache(0x4000, 0x6000).unwrap();
        assert_eq!(run_indirect_call(&mut unicorn), Err(uc_error::FETCH_PROT));
        assert_eq!(unicorn.reg_read(RegisterX86::RIP).unwrap(), 0x4000);
        unicorn.mem_protect(0x4000, 8192, Prot::ALL).unwrap();
        for _ in 0..8 {
            assert_eq!(run_indirect_call(&mut unicorn), Ok(2));
        }
        unicorn.mem_unmap(0x4000, 8192).unwrap();
        assert_eq!(
            run_indirect_call(&mut unicorn),
            Err(uc_error::FETCH_UNMAPPED)
        );
        assert_eq!(unicorn.reg_read(RegisterX86::RIP).unwrap(), 0x4000);
    }

    #[test]
    fn x86_indirect_cache_keeps_hooks_and_instruction_stops_after_invalidation() {
        let mut unicorn = indirect_call_engine(0x4000);
        unicorn
            .mem_write(0x4000, &[0xbb, 42, 0, 0, 0, 0xc3])
            .unwrap();
        for _ in 0..8 {
            assert_eq!(run_indirect_call(&mut unicorn), Ok(42));
        }
        let hook = unicorn
            .add_code_hook(0x4000, 0x4000, |uc, _, _| {
                uc.reg_write(RegisterX86::RBX, 99).unwrap();
                uc.emu_stop().unwrap();
            })
            .unwrap();
        // New hooks do not instrument baseline TBs retroactively. Exercise
        // the existing explicit cache lifecycle, not that separate behavior.
        unicorn.ctl_remove_cache(0x1000, 0x6000).unwrap();
        unicorn.reg_write(RegisterX86::RSP, 0x8800).unwrap();
        unicorn.emu_start(0x1000, 0x100d, 0, 0).unwrap();
        assert_eq!(unicorn.reg_read(RegisterX86::RBX).unwrap(), 99);
        assert_eq!(unicorn.reg_read(RegisterX86::RIP).unwrap(), 0x4000);
        unicorn.remove_hook(hook).unwrap();
        unicorn.ctl_remove_cache(0x1000, 0x6000).unwrap();
        assert_eq!(run_indirect_call(&mut unicorn), Ok(42));
        // Enabling instruction counting installs a hook in the caller too.
        // Flush once, then cover both cold and warm count-limited calls.
        unicorn.ctl_flush_tb().unwrap();
        for _ in 0..8 {
            unicorn.reg_write(RegisterX86::RSP, 0x8800).unwrap();
            unicorn.reg_write(RegisterX86::RBX, 0).unwrap();
            // Only mov rax,target and call rax may execute.
            unicorn.emu_start(0x1000, 0x100d, 0, 2).unwrap();
            assert_eq!(unicorn.reg_read(RegisterX86::RBX).unwrap(), 0);
            assert_eq!(unicorn.reg_read(RegisterX86::RIP).unwrap(), 0x4000);
            assert_eq!(unicorn.reg_read(RegisterX86::RSP).unwrap(), 0x87f8);
        }
    }

    #[test]
    fn x86_cvttss2si_preserves_common_and_exceptional_results() {
        const CODE: u64 = 0x1000;
        let cases = [
            (3.75_f32.to_bits(), 3_u32),
            ((-3.75_f32).to_bits(), (-3_i32) as u32),
            (42.0_f32.to_bits(), 42_u32),
            (0.0_f32.to_bits(), 0_u32),
            ((-0.0_f32).to_bits(), 0_u32),
            (2_147_483_520.0_f32.to_bits(), 0x7fff_ff80),
            ((-2_147_483_648.0_f32).to_bits(), 0x8000_0000),
            (2_147_483_648.0_f32.to_bits(), 0x8000_0000),
            (f32::NAN.to_bits(), 0x8000_0000),
            (f32::INFINITY.to_bits(), 0x8000_0000),
            (f32::NEG_INFINITY.to_bits(), 0x8000_0000),
            (1_u32, 0_u32),
            (0x8000_0001, 0_u32),
        ];

        for (input, expected) in cases {
            let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
            unicorn.mem_map(CODE, 0x1000, Prot::ALL).unwrap();
            // movd xmm0,eax; cvttss2si ecx,xmm0; hlt
            unicorn
                .mem_write(
                    CODE,
                    &[0x66, 0x0f, 0x6e, 0xc0, 0xf3, 0x0f, 0x2c, 0xc8, 0xf4],
                )
                .unwrap();
            unicorn.reg_write(RegisterX86::EAX, input as u64).unwrap();

            unicorn.emu_start(CODE, CODE + 9, 0, 0).unwrap();

            assert_eq!(
                unicorn.reg_read(RegisterX86::ECX).unwrap() as u32,
                expected,
                "input bits {input:#010x}"
            );
        }
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
    fn protected_read_copies_single_and_adjacent_readable_regions() {
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(0x1000, 4096, Prot::READ).unwrap();
        unicorn.mem_map(0x2000, 4096, Prot::READ).unwrap();
        unicorn
            .mem_write(0x1ffc, &[1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap();

        let mut single = [0u8; 4];
        read_protected(&unicorn, 0x1ffc, &mut single).unwrap();
        assert_eq!(single, [1, 2, 3, 4]);

        let mut adjacent = [0u8; 8];
        read_protected(&unicorn, 0x1ffc, &mut adjacent).unwrap();
        assert_eq!(adjacent, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn protected_read_rejects_denied_and_unmapped_ranges_without_partial_copy() {
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(0x1000, 4096, Prot::READ).unwrap();
        unicorn.mem_map(0x2000, 4096, Prot::WRITE).unwrap();
        unicorn
            .mem_write(0x1ffc, &[1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap();

        let mut destination = [0xa5; 8];
        assert_eq!(
            read_protected(&unicorn, 0x1ffc, &mut destination),
            Err(uc_error::READ_PROT)
        );
        assert_eq!(destination, [0xa5; 8]);

        unicorn.mem_unmap(0x2000, 4096).unwrap();
        assert_eq!(
            read_protected(&unicorn, 0x1ffc, &mut destination),
            Err(uc_error::READ_UNMAPPED)
        );
        assert_eq!(destination, [0xa5; 8]);
    }

    #[test]
    fn protected_read_rejects_overflow_without_touching_destination() {
        let unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        let mut destination = [0xa5; 2];
        assert_eq!(
            read_protected(&unicorn, u64::MAX, &mut destination),
            Err(uc_error::READ_UNMAPPED)
        );
        assert_eq!(destination, [0xa5; 2]);
    }

    #[test]
    fn protected_read_stages_cross_region_mmio_when_callback_unmaps_tail() {
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn
            .mmio_map_ro(0x1000, 4096, |unicorn, offset, size| {
                assert_eq!(offset, 4092);
                assert_eq!(size, 4);
                unicorn.mem_unmap(0x2000, 4096).unwrap();
                0x0403_0201
            })
            .unwrap();
        unicorn.mem_map(0x2000, 4096, Prot::READ).unwrap();

        let mut destination = [0xa5; 8];
        assert_eq!(
            read_protected(&unicorn, 0x1ffc, &mut destination),
            Err(uc_error::READ_UNMAPPED)
        );
        assert_eq!(destination, [0xa5; 8]);
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

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn code_hook_restores_thread_global_jit_protection_after_external_change() {
        const CODE: u64 = 0x1000;
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(CODE, 4096, Prot::ALL).unwrap();
        // inc eax; nop
        unicorn.mem_write(CODE, &[0xff, 0xc0, 0x90]).unwrap();
        unicorn
            .add_code_hook(CODE, CODE, |_, _, _| {
                make_current_thread_jit_writable();
            })
            .unwrap();

        unicorn.emu_start(CODE, CODE + 3, 1_000_000, 4).unwrap();
        assert_eq!(unicorn.reg_read(RegisterX86::EAX).unwrap(), 1);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn value_hook_restores_jit_protection_after_nested_api_and_external_change() {
        const CODE: u64 = 0x1000;
        const DATA: u64 = 0x2000;
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(CODE, 4096, Prot::ALL).unwrap();
        // mov byte ptr [rax], 7; hlt
        unicorn.mem_write(CODE, &[0xc6, 0x00, 0x07, 0xf4]).unwrap();
        unicorn.reg_write(RegisterX86::RAX, DATA).unwrap();
        unicorn
            .add_mem_hook(
                HookType::MEM_WRITE_UNMAPPED,
                DATA,
                DATA,
                |unicorn, _, _, _, _| {
                    unicorn
                        .mem_map(DATA, 4096, Prot::READ | Prot::WRITE)
                        .unwrap();
                    make_current_thread_jit_writable();
                    true
                },
            )
            .unwrap();

        unicorn.emu_start(CODE, CODE + 4, 0, 0).unwrap();
        assert_eq!(unicorn.mem_read_as_vec(DATA, 1).unwrap(), [7]);
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
    fn executable_guest_writes_still_invalidate_translated_code() {
        const CODE: u64 = 0x1000;
        const TARGET: u64 = CODE + 0x20;
        const END: u64 = CODE + 0x40;
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        unicorn.mem_map(CODE, 4096, Prot::ALL).unwrap();

        let mut code = vec![0x90; 0x2a];
        // mov byte ptr [rip + 0x1a], 2; jmp TARGET
        code[0..12].copy_from_slice(&[
            0xc6, 0x05, 0x1a, 0x00, 0x00, 0x00, 0x02, 0xe9, 0x14, 0x00, 0x00, 0x00,
        ]);
        // TARGET: mov eax, 1; jmp END
        code[0x20..0x2a]
            .copy_from_slice(&[0xb8, 0x01, 0x00, 0x00, 0x00, 0xe9, 0x16, 0x00, 0x00, 0x00]);
        unicorn.mem_write(CODE, &code).unwrap();
        set_memory_exit_checks(&unicorn, false).unwrap();

        // Translate TARGET before the guest modifies its immediate operand.
        unicorn.emu_start(TARGET, END, 0, 0).unwrap();
        assert_eq!(unicorn.reg_read(RegisterX86::RAX).unwrap(), 1);
        unicorn.reg_write(RegisterX86::RAX, 0).unwrap();

        unicorn.emu_start(CODE, END, 0, 0).unwrap();
        assert_eq!(unicorn.reg_read(RegisterX86::RAX).unwrap(), 2);
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
