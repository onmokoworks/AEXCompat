#[test]
fn legacy_transfer_rect8_ignores_opacity16_and_clips_source_with_anchor_preserved() {
    let mut engine = test_engine(&[0xc3]);
    let (source, _) = argb8_world_fixture(
        &mut engine,
        3,
        1,
        &[255, 11, 0, 0, 255, 22, 0, 0, 255, 33, 0, 0],
    );
    let (destination, destination_pixels) = argb8_world_fixture(&mut engine, 3, 1, &[0; 12]);
    let rect = engine.allocate(16, 4).unwrap();
    let composite = engine.allocate(12, 4).unwrap();
    let mut mode = [0u8; 12];
    mode[8] = 255;
    mode[10..12].copy_from_slice(&59342u16.to_le_bytes());
    engine.write(composite, &mode).unwrap();
    let call = |engine: &mut GuestEngine<'static>, bounds: [i32; 4]| {
        engine
            .write(
                rect,
                &bounds
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        engine
            .call_win64_with_timeout(
                HOST_TRANSFER_RECT8,
                &[1, 0, 0, 0, rect, source, composite, 0, 0, 0, destination],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap()
    };

    assert_eq!(call(&mut engine, [-1, 0, 2, 1]), 0);
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination_pixels, 12)
            .unwrap(),
        [0, 0, 0, 0, 255, 11, 0, 0, 255, 22, 0, 0],
        "source clipping must preserve the requested rect's destination anchor"
    );

    for bounds in [[5, 0, 9, 1], [2, 0, 1, 1]] {
        engine.write(destination_pixels, &[0x5a; 12]).unwrap();
        assert_eq!(call(&mut engine, bounds), 0);
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(destination_pixels, 12)
                .unwrap(),
            [0x5a; 12],
            "empty source intersections must succeed without writing"
        );
    }
}
