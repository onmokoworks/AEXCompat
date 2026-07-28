fn finish_callback(unicorn: &mut Unicorn<'_, GuestState>, result: Result<(), String>) {
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

fn is_scalar_sse_fp(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::Addss
            | Mnemonic::Subss
            | Mnemonic::Mulss
            | Mnemonic::Divss
            | Mnemonic::Sqrtss
            | Mnemonic::Minss
            | Mnemonic::Maxss
            | Mnemonic::Comiss
            | Mnemonic::Ucomiss
            | Mnemonic::Cvtsi2ss
            | Mnemonic::Cvtss2si
            | Mnemonic::Cvttss2si
            | Mnemonic::Addsd
            | Mnemonic::Subsd
            | Mnemonic::Mulsd
            | Mnemonic::Divsd
            | Mnemonic::Sqrtsd
            | Mnemonic::Minsd
            | Mnemonic::Maxsd
            | Mnemonic::Comisd
            | Mnemonic::Ucomisd
            | Mnemonic::Cvtsi2sd
            | Mnemonic::Cvtsd2si
            | Mnemonic::Cvttsd2si
    )
}

fn coalesce_census_extents(
    blocks: &[CensusBlock],
    image_base: u64,
    total_dynamic_instructions: u64,
) -> Vec<CensusExtent> {
    let mut by_address = blocks.iter().collect::<Vec<_>>();
    by_address.sort_by_key(|block| (block.address, block.size_bytes));
    let mut extents: Vec<CensusExtent> = Vec::new();
    for block in by_address {
        let block_end = block.address + u64::from(block.size_bytes);
        if let Some(extent) = extents.last_mut().filter(|extent| {
            // Adjacent or overlapping translated blocks belong to one
            // promotable guest-code region. QEMU may split the same bytes into
            // several block variants depending on the incoming branch.
            block.address <= extent.end_address
        }) {
            extent.end_address = extent.end_address.max(block_end);
            extent.end_rva = extent.end_address - image_base;
            extent.size_bytes = extent.end_address - extent.start_address;
            extent.block_variants += 1;
            extent.dynamic_instructions = extent
                .dynamic_instructions
                .saturating_add(block.dynamic_instructions);
        } else {
            extents.push(CensusExtent {
                start_address: block.address,
                end_address: block_end,
                start_rva: block.address - image_base,
                end_rva: block_end - image_base,
                size_bytes: block_end - block.address,
                block_variants: 1,
                dynamic_instructions: block.dynamic_instructions,
                dynamic_instruction_fraction: 0.0,
            });
        }
    }
    for extent in &mut extents {
        extent.dynamic_instruction_fraction = if total_dynamic_instructions == 0 {
            0.0
        } else {
            extent.dynamic_instructions as f64 / total_dynamic_instructions as f64
        };
    }
    extents.sort_by_key(|extent| std::cmp::Reverse(extent.dynamic_instructions));
    extents
}

