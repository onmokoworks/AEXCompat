//! Safe host-local civil time conversion; never accepts guest pointers.
pub fn localtime_fields(seconds: i64) -> Result<[i32; 9], String> {
    let timestamp: libc::time_t = seconds
        .try_into()
        .map_err(|_| "host time_t range exceeded")?;
    let mut value = std::mem::MaybeUninit::<libc::tm>::uninit();
    // Reentrant host conversion: no guest pointer crosses the FFI boundary.
    #[cfg(unix)]
    let success = unsafe { !libc::localtime_r(&timestamp, value.as_mut_ptr()).is_null() };
    #[cfg(windows)]
    let success = unsafe { libc::localtime_s(value.as_mut_ptr(), &timestamp) == 0 };
    if !success {
        return Err("host local time conversion failed".into());
    }
    let value = unsafe { value.assume_init() };
    Ok([
        value.tm_sec,
        value.tm_min,
        value.tm_hour,
        value.tm_mday,
        value.tm_mon,
        value.tm_year,
        value.tm_wday,
        value.tm_yday,
        value.tm_isdst,
    ])
}

#[cfg(windows)]
unsafe extern "C" {
    fn _mktime64(tm: *mut libc::tm) -> libc::time_t;
}

/// Windows CRT timezone (standard-time minutes west of UTC) and current DST flag.
pub fn timeb_zone(seconds: i64) -> Result<(i16, i16), String> {
    let timestamp: libc::time_t = seconds
        .try_into()
        .map_err(|_| "host time_t range exceeded")?;
    let local = localtime_fields(seconds)?;
    let mut utc = std::mem::MaybeUninit::<libc::tm>::uninit();
    #[cfg(unix)]
    let success = unsafe { !libc::gmtime_r(&timestamp, utc.as_mut_ptr()).is_null() };
    #[cfg(windows)]
    let success = unsafe { libc::gmtime_s(utc.as_mut_ptr(), &timestamp) == 0 };
    if !success {
        return Err("host UTC conversion failed".into());
    }
    let mut utc = unsafe { utc.assume_init() };
    // Interpret UTC civil fields as local STANDARD time. Forcing tm_isdst=0
    // removes the actual DST adjustment, including non-hour seasonal shifts.
    utc.tm_isdst = 0;
    #[cfg(unix)]
    let standard = unsafe { libc::mktime(&mut utc) };
    // The libc crate does not bind `mktime` on Windows: the UCRT header makes
    // it an inline wrapper over the exported `_mktime64`, which is what a
    // 64-bit `time_t` resolves to.
    #[cfg(windows)]
    let standard = unsafe { _mktime64(&mut utc) };
    if standard == -1 {
        return Err("host standard time conversion failed".into());
    }
    let west = (standard as i64)
        .checked_sub(seconds)
        .ok_or("timezone difference overflow")?;
    if west % 60 != 0 {
        return Err("sub-minute host timezone is not representable".into());
    }
    let west = i16::try_from(west / 60).map_err(|_| "timezone minutes range exceeded")?;
    if local[8] < 0 {
        return Err("host daylight saving state is unknown".into());
    }
    Ok((west, i16::from(local[8] > 0)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_time_respects_zone_and_daylight_saving() {
        const MARKER: &str = "AEX_HOST_TIME_TEST_ZONE";
        if let Ok(zone) = std::env::var(MARKER) {
            let expected = match zone.as_str() {
                "UTC0" => [
                    [0, 0, 0, 1, 0, 124, 1, 0, 0],
                    [0, 0, 0, 1, 6, 124, 1, 182, 0],
                ],
                "LHST-10:30LHDT-11,M10.1.0,M4.1.0" => [
                    [0, 0, 11, 1, 0, 124, 1, 0, 1],
                    [0, 30, 10, 1, 6, 124, 1, 182, 0],
                ],
                "JST-9" => [
                    [0, 0, 9, 1, 0, 124, 1, 0, 0],
                    [0, 0, 9, 1, 6, 124, 1, 182, 0],
                ],
                "IST-5:30" => [
                    [0, 30, 5, 1, 0, 124, 1, 0, 0],
                    [0, 30, 5, 1, 6, 124, 1, 182, 0],
                ],
                "PST8PDT" => [
                    [0, 0, 16, 31, 11, 123, 0, 364, 0],
                    [0, 0, 17, 30, 5, 124, 0, 181, 1],
                ],
                _ => panic!("unexpected test zone"),
            };
            for (seconds, expected) in [1_704_067_200, 1_719_792_000].into_iter().zip(expected) {
                assert_eq!(super::localtime_fields(seconds).unwrap(), expected);
                let west = match zone.as_str() {
                    "UTC0" => 0,
                    "JST-9" => -540,
                    "IST-5:30" => -330,
                    "LHST-10:30LHDT-11,M10.1.0,M4.1.0" => -630,
                    "PST8PDT" => 480,
                    _ => unreachable!(),
                };
                assert_eq!(
                    super::timeb_zone(seconds).unwrap(),
                    (west, expected[8] as i16)
                );
            }
            return;
        }
        // Isolated processes avoid mutating the test runner's global timezone.
        // The Microsoft CRT reads TZ as `tzn[+|-]hh[:mm[:ss]][dzn]` and applies
        // US daylight-saving rules to any `dzn`; it ignores POSIX `,M...`
        // transition rules and falls back to UTC, so the southern-hemisphere
        // zone is only meaningful where TZ is POSIX. IST-5:30 keeps a
        // non-hour offset covered on every host.
        let mut zones = vec!["UTC0", "PST8PDT", "JST-9", "IST-5:30"];
        if cfg!(not(windows)) {
            zones.push("LHST-10:30LHDT-11,M10.1.0,M4.1.0");
        }
        for zone in zones {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::local_time_respects_zone_and_daylight_saving",
                    "--nocapture",
                ])
                .env(MARKER, zone)
                .env("TZ", zone)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}
