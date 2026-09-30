#[test]
fn iterate16_progress_matches_forward_reverse_and_degenerate_contract() {
    const CODE: u64 = 0x1000_0000;
    const PROGRESS: u64 = CODE + 0x40;
    let pixel = [
        0x48, 0x8b, 0x44, 0x24, 0x28, 0xc7, 0x00, 0x01, 0x02, 0x03, 0x04, 0x31, 0xc0, 0xc3,
    ];
    // Append each current/total pair to effect_ref using the count at +32.
    let progress = [
        0x8b, 0x41, 0x20, 0x89, 0x14, 0xc1, 0x44, 0x89, 0x44, 0xc1, 0x04, 0xff, 0x41, 0x20, 0x31,
        0xc0, 0xc3,
    ];
    let mut code = vec![0x90; 0x60];
    code[..pixel.len()].copy_from_slice(&pixel);
    code[0x40..0x40 + progress.len()].copy_from_slice(&progress);
    let mut engine = test_engine(&code);
    let destination_pixels = engine.allocate(16, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    world[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&destination_pixels.to_le_bytes());
    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    engine.write(destination_world, &world).unwrap();
    let observations = engine.allocate(36, 4).unwrap();
    let in_data = engine.allocate(abi::PF_IN_DATA_SIZE, 8).unwrap();
    let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
    input[abi::INTER_PROGRESS_OFFSET..abi::INTER_PROGRESS_OFFSET + 8]
        .copy_from_slice(&PROGRESS.to_le_bytes());
    input[abi::IN_EFFECT_REF_OFFSET..abi::IN_EFFECT_REF_OFFSET + 8]
        .copy_from_slice(&observations.to_le_bytes());
    engine.write(in_data, &input).unwrap();

    let call = |engine: &mut GuestEngine<'static>, base: i32, final_value: i32| {
        engine.call_win64_with_timeout(
            HOST_ITERATE16,
            &[
                in_data,
                base as u32 as u64,
                final_value as u32 as u64,
                0,
                0,
                0,
                CODE,
                destination_world,
            ],
            TIMEOUT_MICROSECONDS,
        )
    };
    let read = |engine: &GuestEngine<'static>| {
        let mut bytes = [0u8; 36];
        engine.read(observations, &mut bytes).unwrap();
        bytes
            .chunks_exact(4)
            .map(|value| i32::from_le_bytes(value.try_into().unwrap()))
            .collect::<Vec<_>>()
    };

    assert_eq!(call(&mut engine, 10, 14).unwrap(), 0);
    assert_eq!(read(&engine), [12, 14, 14, 14, 0, 0, 0, 0, 2]);

    engine.write(observations, &[0; 36]).unwrap();
    assert_eq!(call(&mut engine, 14, 10).unwrap(), 0);
    assert_eq!(read(&engine), [2, 4, 4, 4, 0, 0, 0, 0, 2]);

    engine.write(observations, &[0; 36]).unwrap();
    engine.write(destination_pixels, &[0; 16]).unwrap();
    assert_eq!(call(&mut engine, 0, 0).unwrap(), 0);
    assert_eq!(read(&engine), [0; 9]);
    let mut pixels = [0u8; 16];
    engine.read(destination_pixels, &mut pixels).unwrap();
    assert_eq!(pixels, [1, 2, 3, 4, 0, 0, 0, 0, 1, 2, 3, 4, 0, 0, 0, 0]);
}
