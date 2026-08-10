#[test]
fn legacy_area_sample8_ignores_area_and_maps_edge_behaviors() {
    let mut engine = test_engine(&[0xc3]);
    let (params, destination, _) = argb8_sampling_fixture(&mut engine);
    engine.write(params, &0x8000i32.to_le_bytes()).unwrap();
    engine.write(params + 4, &0x8000i32.to_le_bytes()).unwrap();
    engine
        .write(params + 8, &(-0x1234_567i32).to_le_bytes())
        .unwrap();

    let sample = |engine: &mut GuestEngine<'static>, edge: u32| {
        engine.write(params + 24, &edge.to_le_bytes()).unwrap();
        engine.write(destination, &[0x5a; 4]).unwrap();
        let result = engine
            .call_win64(
                HOST_AREA_SAMPLE8,
                [
                    0,
                    (-0x8000i32) as u32 as u64,
                    0x8000,
                    params,
                    destination,
                    0,
                ],
            )
            .unwrap();
        let pixel = engine.unicorn.mem_read_as_vec(destination, 4).unwrap();
        (result, pixel)
    };

    assert_eq!(sample(&mut engine, 0), (0, vec![128, 0, 50, 0]));
    assert_eq!(sample(&mut engine, 1), (0, vec![255, 0, 50, 0]));
    assert_eq!(sample(&mut engine, 2), (0, vec![255, 50, 50, 0]));
    assert_eq!(
        sample(&mut engine, 0xfeed_beef),
        (0, vec![128, 0, 50, 0]),
        "unknown edge values must degrade to ZERO"
    );

    engine.write(params, &0i32.to_le_bytes()).unwrap();
    engine.write(destination, &[0x5a; 4]).unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_AREA_SAMPLE8,
                [
                    0,
                    (-0x8000i32) as u32 as u64,
                    0x8000,
                    params,
                    destination,
                    0
                ],
            )
            .unwrap(),
        4
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        [0x5a; 4],
        "invalid radii must remain fail-closed without writing"
    );
}
