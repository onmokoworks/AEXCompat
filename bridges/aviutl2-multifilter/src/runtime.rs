/// The AEX layer-parameter slots (`PF_Param_LAYER`, reported by discovery as
/// `kind == "layer"`) in declaration order. Taken from the raw discovery
/// parameters: `build_item` never maps a layer into a config item, so the
/// registered `defaults` cannot contain one.
#[doc(hidden)]
pub fn layer_slots_of(parameters: &[InteractiveParameter]) -> Vec<u32> {
    parameters
        .iter()
        .filter(|parameter| parameter.kind == "layer")
        .map(|parameter| parameter.slot)
        .collect()
}

/// AviUtl2's single global virtual buffer, written by the "仮想バッファ出力"
/// (virtual buffer output / output switch) filter and read here as an AEX layer
/// input (issue #645). Named `tempbuffer` in the image-resource namespace.
const VIRTUAL_BUFFER_NAME: &str = "tempbuffer";

/// Reads the virtual buffer back to the CPU as RGBA8, or `None` if nothing is
/// written to it this frame (no "仮想バッファ出力" upstream) or it cannot be read.
///
/// The virtual buffer is a GPU-only `DXGI_FORMAT_R16G16B16A16_FLOAT` texture;
/// `get_image_resource_data` fails on it for every format (verified — it works
/// only for the ordinary `object` resource). So take its `ID3D11Texture2D`, copy
/// it into a CPU-readable STAGING texture, `Map` it, and convert the half-float
/// RGBA to RGBA8, honoring the mapped row pitch (which is padded).
fn read_virtual_buffer_rgba8(video: *mut FILTER_PROC_VIDEO) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT;
    use windows::core::Interface;

    let name: Vec<u16> = VIRTUAL_BUFFER_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `video` is the non-null FILTER_PROC_VIDEO the host passed to
    // proc_video; the returned texture pointer is host-owned and valid until the
    // proc returns, so it is only borrowed (never released) below.
    let tex_ptr = unsafe { ((*video).get_image_resource_texture2d)(name.as_ptr()) };
    if tex_ptr.is_null() {
        return None;
    }
    let src: &ID3D11Texture2D = unsafe { ID3D11Texture2D::from_raw_borrowed(&tex_ptr) }?;

    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        src.GetDesc(&mut desc);
        // Only the format the virtual buffer is known to use; a different format
        // would need a different unpack, so skip rather than misread it.
        if desc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT || desc.Width == 0 || desc.Height == 0 {
            return None;
        }
        let (width, height) = (desc.Width, desc.Height);

        let device: ID3D11Device = src.GetDevice().ok()?;
        let ctx: ID3D11DeviceContext = device.GetImmediateContext().ok()?;

        let mut staging_desc = desc;
        staging_desc.Usage = D3D11_USAGE_STAGING;
        staging_desc.BindFlags = 0;
        staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        staging_desc.MiscFlags = 0;
        let mut staging: Option<ID3D11Texture2D> = None;
        device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .ok()?;
        let staging = staging?;

        ctx.CopyResource(&staging, src);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .ok()?;
        let row_pitch = mapped.RowPitch as usize;
        if row_pitch < width as usize * 8 {
            ctx.Unmap(&staging, 0);
            return None;
        }
        // Claim only up to the last row's pixel data: D3D11 specifies RowPitch
        // as the row stride, not that the final row's padding is readable.
        let mapped_len = row_pitch * (height as usize - 1) + width as usize * 8;
        let mapped_bytes = std::slice::from_raw_parts(mapped.pData as *const u8, mapped_len);
        let rgba = unpack_rgba16f_to_rgba8(mapped_bytes, width, height, row_pitch);
        ctx.Unmap(&staging, 0);

        rgba.map(|rgba| (width, height, rgba))
    }
}

/// Unpacks a mapped `R16G16B16A16_FLOAT` image (RGBA order, little-endian half
/// floats, `row_pitch`-padded rows) into packed RGBA8, clamping each channel to
/// [0, 1] (a displacement map's values are normalized; NaN clamps to 0). Returns
/// `None` if `bytes` is too short for the claimed geometry.
fn unpack_rgba16f_to_rgba8(
    bytes: &[u8],
    width: u32,
    height: u32,
    row_pitch: usize,
) -> Option<Vec<u8>> {
    let (width, height) = (width as usize, height as usize);
    if width == 0 || height == 0 {
        return None;
    }
    // The last row only needs its pixel data, not its padding (matching what
    // the D3D11 mapping is claimed to expose).
    if row_pitch < width * 8 || bytes.len() < row_pitch * (height - 1) + width * 8 {
        return None;
    }
    // Written row by row into a pre-sized buffer: the previous per-channel
    // `Vec::push` with bounds-checked indexing measured 21 ms for one 1080p
    // frame (issue #674), which is ~17x the memory bandwidth floor for the
    // 24 MB it moves. The scalar row below is the behavioural reference; the
    // F16C path must agree with it byte for byte (pinned by a test).
    let mut rgba = vec![0u8; width * height * 4];
    let simd = f16c_row_available();
    for y in 0..height {
        let source = &bytes[y * row_pitch..y * row_pitch + width * 8];
        let target = &mut rgba[y * width * 4..(y + 1) * width * 4];
        if simd {
            // SAFETY: guarded by the runtime feature detection above, and the
            // slices are exactly `width` pixels wide by construction.
            unsafe { unpack_row_f16c(source, target) };
        } else {
            unpack_row_scalar(source, target);
        }
    }
    Some(rgba)
}

/// One pixel row, `width` RGBA half-float pixels to RGBA8. The reference
/// conversion: clamp to [0, 1] (NaN clamps to 0), scale by 255, round half up.
fn unpack_row_scalar(source: &[u8], target: &mut [u8]) {
    for (pixel, out) in source.chunks_exact(8).zip(target.chunks_exact_mut(4)) {
        for channel in 0..4 {
            let value =
                half::f16::from_le_bytes([pixel[channel * 2], pixel[channel * 2 + 1]]).to_f32();
            out[channel] = (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
}

/// Whether this CPU can run [`unpack_row_f16c`]. `half`'s own conversion only
/// uses the F16C instruction when the *compile-time* target has it, and the
/// x86-64 baseline does not, so the released plug-in converts in software.
/// Detecting at runtime keeps the baseline where it is while letting every
/// machine since Ivy Bridge take the hardware path.
fn f16c_row_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            std::arch::is_x86_feature_detected!("f16c")
                && std::arch::is_x86_feature_detected!("avx")
        })
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Eight half floats per iteration through `vcvtph2ps`, then the same clamp,
/// scale and round-half-up the scalar row applies, packed back to bytes.
///
/// # Safety
/// The caller must have confirmed F16C and AVX support ([`f16c_row_available`]).
/// `source` must hold `target.len() / 4` pixels of 8 bytes each.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "f16c,avx")]
unsafe fn unpack_row_f16c(source: &[u8], target: &mut [u8]) {
    use std::arch::x86_64::{
        __m128i, _mm_cvttps_epi32, _mm_loadu_si128, _mm_packs_epi32, _mm_packus_epi16,
        _mm_storel_epi64, _mm256_add_ps, _mm256_castps256_ps128, _mm256_cvtph_ps,
        _mm256_extractf128_ps, _mm256_max_ps, _mm256_min_ps, _mm256_mul_ps, _mm256_set1_ps,
        _mm256_setzero_ps,
    };

    // Two pixels (8 channels) per iteration; the remainder falls back to the
    // reference row so odd widths stay byte-identical.
    let vectorized = target.len() / 8 * 8;
    let zero = _mm256_setzero_ps();
    let one = _mm256_set1_ps(1.0);
    let scale = _mm256_set1_ps(255.0);
    let half_step = _mm256_set1_ps(0.5);
    let mut offset = 0;
    while offset < vectorized {
        let packed = _mm_loadu_si128(source.as_ptr().add(offset * 2) as *const __m128i);
        let values = _mm256_cvtph_ps(packed);
        // max-then-min with the constant second: `_mm256_max_ps` returns its
        // second operand for a NaN input, so NaN lands on 0 exactly as
        // `f32::clamp` followed by a saturating `as u8` does in the reference.
        let clamped = _mm256_min_ps(_mm256_max_ps(values, zero), one);
        let scaled = _mm256_add_ps(_mm256_mul_ps(clamped, scale), half_step);
        // Truncation after +0.5 is round-half-up, matching `as u8` above;
        // the default rounding mode would round half to even instead.
        let low = _mm_cvttps_epi32(_mm256_castps256_ps128(scaled));
        let high = _mm_cvttps_epi32(_mm256_extractf128_ps(scaled, 1));
        let words = _mm_packs_epi32(low, high);
        let bytes = _mm_packus_epi16(words, words);
        _mm_storel_epi64(target.as_mut_ptr().add(offset) as *mut __m128i, bytes);
        offset += 8;
    }
    if offset < target.len() {
        unpack_row_scalar(&source[offset * 2..], &mut target[offset..]);
    }
}

/// Build one leaked FILTER_ITEM for an exposed parameter, plus a reader and the
/// (range-normalized) parameter to send. Mirrors the aviutl2 bridge's mapping.
fn build_item(
    parameter: &InteractiveParameter,
    item_name: &str,
) -> Option<(*const c_void, ItemReader, InteractiveParameter)> {
    match parameter.kind.as_str() {
        "float" => {
            let (min, max) = bounded_range(parameter)?;
            let ptr = leak_track(item_name, parameter.value, min, max, track_step(max - min));
            let mut sent = parameter.clone();
            sent.value = sent.value.clamp(min, max);
            Some((
                ptr as *const c_void,
                ItemReader::Track {
                    ptr,
                    slot: parameter.slot,
                    integer: false,
                },
                sent,
            ))
        }
        "integer" => {
            if !parameter.choices.is_empty() {
                // Popup -> dropdown (AE popups are 1-based).
                let count = parameter.choices.len() as i32;
                let selected = (parameter.value as i32).clamp(1, count);
                let ptr = leak_select(item_name, selected, &parameter.choices);
                let mut sent = parameter.clone();
                sent.minimum = 1.0;
                sent.maximum = count as f64;
                sent.value = f64::from(selected);
                return Some((
                    ptr as *const c_void,
                    ItemReader::Select {
                        ptr,
                        slot: parameter.slot,
                    },
                    sent,
                ));
            }
            let (min, max) = bounded_range(parameter)?;
            if min == 0.0 && max == 1.0 {
                let ptr = leak_checkbox(item_name, parameter.value != 0.0);
                let mut sent = parameter.clone();
                sent.value = f64::from(parameter.value != 0.0);
                Some((
                    ptr as *const c_void,
                    ItemReader::Checkbox {
                        ptr,
                        slot: parameter.slot,
                    },
                    sent,
                ))
            } else {
                let ptr = leak_track(item_name, parameter.value.round(), min, max, 1.0);
                let mut sent = parameter.clone();
                sent.value = sent.value.round().clamp(min, max);
                Some((
                    ptr as *const c_void,
                    ItemReader::Track {
                        ptr,
                        slot: parameter.slot,
                        integer: true,
                    },
                    sent,
                ))
            }
        }
        "color" => {
            // InteractiveParameter.color is ARGB; AviUtl2 color code is 0x00RRGGBB.
            let (r, g, b) = (parameter.color[1], parameter.color[2], parameter.color[3]);
            let ptr = leak_color(item_name, r, g, b);
            Some((
                ptr as *const c_void,
                ItemReader::Color {
                    ptr,
                    slot: parameter.slot,
                },
                parameter.clone(),
            ))
        }
        // "angle" and others are not exposed (stay at the AEX default).
        _ => None,
    }
}

/// Produces the names used by AviUtl2's config items.
///
/// AviUtl2 persists config values by item name, while AEX parameter names are
/// only human-facing labels and are not required to be unique. Keep the old
/// name for a unique visible parameter, but add its stable AEX slot to every
/// duplicate. Empty labels get the same slot-based fallback. The final set is
/// checked again so a user-supplied label cannot collide with a generated one.
fn unique_item_names(parameters: &[InteractiveParameter]) -> Vec<Option<String>> {
    let mut counts = HashMap::<String, usize>::new();
    for parameter in parameters.iter().filter(|parameter| parameter.visible) {
        let name = parameter.name.trim();
        if !name.is_empty() {
            *counts.entry(name.to_owned()).or_default() += 1;
        }
    }

    let mut used = HashSet::<String>::new();
    parameters
        .iter()
        .map(|parameter| {
            if !parameter.visible {
                return None;
            }
            let trimmed = parameter.name.trim();
            let base = if trimmed.is_empty() {
                format!("Parameter {}", parameter.slot)
            } else {
                parameter.name.clone()
            };
            let duplicate =
                !trimmed.is_empty() && counts.get(trimmed).copied().unwrap_or_default() > 1;
            let stem = if duplicate || trimmed.is_empty() {
                format!("{base} [slot {}]", parameter.slot)
            } else {
                base
            };
            let mut candidate = stem.clone();
            let mut disambiguator = 2u32;
            while !used.insert(candidate.clone()) {
                candidate = format!("{stem} [{disambiguator}]");
                disambiguator = disambiguator.saturating_add(1);
            }
            Some(candidate)
        })
        .collect()
}

fn leak_track(name: &str, value: f64, min: f64, max: f64, step: f64) -> *const FILTER_ITEM_TRACK {
    Box::leak(Box::new(FILTER_ITEM_TRACK {
        r#type: wide_leak("track2"),
        name: wide_leak(name),
        value: value.clamp(min, max),
        s: min,
        e: max,
        step,
        zero_display: std::ptr::null(),
        slider_ratio: 1.0,
    }))
}

fn leak_checkbox(name: &str, value: bool) -> *const FILTER_ITEM_CHECKBOX {
    Box::leak(Box::new(FILTER_ITEM_CHECKBOX {
        r#type: wide_leak("check"),
        name: wide_leak(name),
        value,
    }))
}

fn leak_select(name: &str, value: i32, choices: &[String]) -> *const FILTER_ITEM_SELECT {
    let mut list: Vec<FILTER_ITEM_SELECT_ITEM> = choices
        .iter()
        .enumerate()
        .map(|(index, label)| FILTER_ITEM_SELECT_ITEM {
            name: wide_leak(label),
            value: index as i32 + 1,
        })
        .collect();
    // Null-name terminator.
    list.push(FILTER_ITEM_SELECT_ITEM {
        name: std::ptr::null(),
        value: 0,
    });
    let items = Box::leak(list.into_boxed_slice()).as_ptr();
    Box::leak(Box::new(FILTER_ITEM_SELECT {
        r#type: wide_leak("select"),
        name: wide_leak(name),
        value,
        items,
    }))
}

fn leak_color(name: &str, r: u8, g: u8, b: u8) -> *const FILTER_ITEM_COLOR {
    Box::leak(Box::new(FILTER_ITEM_COLOR {
        r#type: wide_leak("color"),
        name: wide_leak(name),
        value: FILTER_ITEM_COLOR_VALUE { bgrx: [b, g, r, 0] },
    }))
}

fn bounded_range(parameter: &InteractiveParameter) -> Option<(f64, f64)> {
    let (min, max) = (parameter.minimum, parameter.maximum);
    (min.is_finite() && max.is_finite() && min < max).then_some((min, max))
}

fn track_step(span: f64) -> f64 {
    for step in [1.0, 0.1, 0.01] {
        if span / step >= 100.0 {
            return step;
        }
    }
    0.001
}

// --- Per-frame render ----------------------------------------------------

unsafe extern "C" fn render_callback(
    _cif: &low::ffi_cif,
    result: &mut u8,
    args: *const *const c_void,
    userdata: &FilterCtx,
) {
    let video = unsafe { *(*args as *const *mut FILTER_PROC_VIDEO) };
    // Never let a panic unwind across the C boundary (that aborts AviUtl2). On
    // panic report failure and leave the frame's pixels unchanged.
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_frame(userdata, video)
    }))
    .unwrap_or(false);
    *result = ok as u8;
}

fn render_frame(ctx: &FilterCtx, video: *mut FILTER_PROC_VIDEO) -> bool {
    if video.is_null() {
        return false;
    }
    let object: *const OBJECT_INFO = unsafe { (*video).object };
    let scene: *const SCENE_INFO = unsafe { (*video).scene };
    if object.is_null() || scene.is_null() {
        return false;
    }
    let width = unsafe { (*object).width };
    let height = unsafe { (*object).height };
    if width <= 0 || height <= 0 {
        return true;
    }
    let (width, height) = (width as u32, height as u32);
    if width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return true; // leave pixels unchanged
    }

    let rate = unsafe { (*scene).rate };
    let scale = unsafe { (*scene).scale };
    if rate <= 0 || scale <= 0 {
        return false;
    }
    let frame = unsafe { (*object).frame };
    let frame_total = unsafe { (*object).frame_total };
    let current_time = (frame as i64 * scale as i64).clamp(0, i32::MAX as i64) as i32;
    let total_time = ((frame_total.max(1)) as i64 * scale as i64).clamp(1, i32::MAX as i64) as i32;
    let time_scale = rate as u32;
    let time_step = scale;

    // Current pixels (RGBA8, packed).
    let count = (width as usize) * (height as usize);
    let mut pixels: Vec<PIXEL_RGBA> = (0..count)
        .map(|_| PIXEL_RGBA {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        })
        .collect();
    unsafe { ((*video).get_image_data)(pixels.as_mut_ptr()) };
    let rgba = pixels_to_bytes(&pixels);

    // Overlay this frame's current config values onto the exposed defaults,
    // then transport only actual changes. An untouched UI must leave the
    // PARAMS_SETUP definitions untouched, matching AE's parameter contract.
    let parameters = if ctx.defaults.is_empty() {
        None
    } else {
        let mut values = ctx.defaults.clone();
        apply_readers(&mut values, &ctx.readers);
        let changed = changed_interactive_parameters(&ctx.defaults, &values);
        (!changed.is_empty()).then_some(changed)
    };

    let effect_id = unsafe { (*object).effect_id };
    let identity = GeomIdentity {
        width,
        height,
        time_step,
        total_time,
        time_scale,
    };

    // Reuse a live matching session, else open one outside the map/pool lock
    // (open blocks for seconds spawning the worker). The blocking render
    // round-trip below runs off-lock too, so concurrent objects/threads never
    // serialize on the map. An AEX sharing its dependency closure with other
    // registered AEXes routes through the pooled cluster session (issue
    // #405); everything else keeps the per-effect session.
    // Read once, only if this session actually opens: map AviUtl2's virtual
    // buffer onto the AEX's first layer parameter (issue #645). Empty when the
    // AEX has no layer input or nothing is written to the virtual buffer.
    let read_layers = || -> Vec<SessionLayer> {
        let Some(&slot) = ctx.layer_slots.first() else {
            return Vec::new();
        };
        match read_virtual_buffer_rgba8(video) {
            Some((layer_width, layer_height, rgba)) => vec![SessionLayer {
                slot,
                width: layer_width,
                height: layer_height,
                rgba,
                timed: None,
                // Opened dynamic so the map can follow a moving scene (issue
                // #674). The geometry a session opens with is the geometry it
                // keeps: a virtual buffer that changes size needs a new
                // session, which a changed object geometry already forces.
                dynamic: true,
            }],
            None => Vec::new(),
        }
    };
    let route = match route_session(ctx, effect_id, &identity, &read_layers) {
        Ok(route) => route,
        Err(()) => return false,
    };
    let (tx, plugin_index) = match &route {
        SessionRoute::PerEffect(tx, _) => (tx.clone(), 0),
        SessionRoute::Pooled(tx, _, plugin_index, _) => (tx.clone(), *plugin_index),
    };

    // Re-read the virtual buffer for this frame, so a moving scene moves the map
    // (issue #674). Only when this AEX has a layer slot: without one the
    // readback is pure cost. A buffer that cannot be read this frame leaves the
    // layer as it was rather than blanking it, and one whose size no longer
    // matches the session is refused by the update below, which drops the
    // session so the next frame reopens at the new geometry.
    let layer = ctx
        .layer_slots
        .first()
        .and_then(|&slot| read_virtual_buffer_rgba8(video).map(|(_, _, rgba)| (slot, rgba)));
    match render_on(&tx, plugin_index, current_time, rgba, parameters, layer) {
        FrameReply::Rendered(frame) => {
            // The frame is published at whatever size it came back as, which is
            // not always the object's. A SmartFX effect that grows its output -
            // a glow reaching past the layer - answers larger, and set_image_data
            // takes the size as an argument ("width,height: 画像サイズ" in the
            // filter2 API), so the grown frame goes through unchanged.
            //
            // This used to be refused on the stated grounds that "a filter object
            // cannot change the image size". Nothing checked that: the frame was
            // dropped and the object kept its old pixels, so DeepGlow2 rendered
            // correctly every frame and nothing reached the screen (#914).
            // The bounds the input is held to apply to the output too, rather
            // than being left to the broker's matching constants: what this
            // hands `set_image_data` is this crate's invariant to keep.
            let out = bytes_to_pixels(&frame.pixels);
            let pixels = u64::from(frame.width) * u64::from(frame.height);
            if frame.width == 0
                || frame.height == 0
                || frame.width > MAX_DIMENSION
                || frame.height > MAX_DIMENSION
                || pixels > MAX_PIXELS
                || out.len() as u64 != pixels
            {
                // Unreachable while the broker validates the frame it sends,
                // which is exactly why it must say something if it happens -
                // dropping a frame in silence is how #914 stayed invisible.
                report_frame_trouble(
                    &ctx.plugin,
                    FrameTrouble::Refused("the frame's size is outside this bridge's bounds"),
                );
                return true;
            }
            // Says so once when a filter that had been failing renders again;
            // silent for one that never stopped. After the refusal above, not
            // before it: a filter refused on every frame would otherwise clear
            // its own trouble state each time, so the once-per-60 collapse
            // never engaged and every frame printed both "rendering again" and
            // the refusal - at preview frame rate, and the first of the two
            // untrue.
            report_frame_recovered(&ctx.plugin);
            unsafe {
                ((*video).set_image_data)(out.as_ptr(), frame.width as i32, frame.height as i32)
            };
            true
        }
        // Keep the session; leave this frame's pixels. Saying so matters: with
        // the frame's pixels left alone, an effect erroring on every frame and
        // an effect doing nothing look identical on screen (issue #697).
        FrameReply::FrameLocal(code, said) => {
            report_frame_trouble(&ctx.plugin, FrameTrouble::Error(code, said.as_deref()));
            true
        }
        FrameReply::SessionLost(reason) => {
            report_frame_trouble(&ctx.plugin, FrameTrouble::SessionLost(&reason));
            // Drop this exact instance so the next frame reopens, without
            // disturbing a healthy session a concurrent reopen may have installed.
            match route {
                SessionRoute::PerEffect(_, serial) => {
                    remove_session(&ctx.sessions, effect_id, serial)
                }
                SessionRoute::Pooled(_, serial, _, key) => pool_remove(&key, serial),
            }
            true
        }
    }
}

/// What went wrong with one frame, for [`report_frame_trouble`].
enum FrameTrouble<'a> {
    /// The plug-in (or the host on its behalf) returned an error for this
    /// frame. The session stays usable. The second field is the plug-in's own
    /// account of it, when it left one (issue #707).
    Error(i64, Option<&'a str>),
    /// The session is gone; the next frame opens a fresh one.
    SessionLost(&'a str),
    /// The frame arrived but this bridge would not hand it to AviUtl2. The
    /// session stays usable; the object keeps the pixels it had.
    Refused(&'a str),
}

/// The name AE's own headers give a selector's return code, so a reader does not
/// have to look up a bare number. Codes outside both enumerations - a plug-in's
/// own, or one of the host's negative internal ones - keep just their number.
///
/// The 512 block is `PF_Err`, not an effect's private codes: `AE_Effect.h`
/// defines `PF_FIRST_ERR` as 512 and numbers `PF_Err_INTERNAL_STRUCT_DAMAGED`
/// onward from it by ordinal. Issue #704 recorded the opposite - "the SDK's
/// PF_Err / A_Err enumerations do not have this value", with a hypothesis that
/// Adobe's own effects define private codes based at 512 - because `A_Err`,
/// which stops at 13, was the enumeration consulted. Its 41 plug-ins answering
/// 512 are answering `PF_Err_INTERNAL_STRUCT_DAMAGED`.
///
/// Below 512 only what a *selector* can return is named. `PF_Err` defines
/// exactly `NONE` (0), `OUT_OF_MEMORY` (4) and the 512 block, so a small code
/// is otherwise the plug-in's own and keeps just its number. The one exception
/// is 13: `A_Err_MISSING_SUITE` reaches a selector's return through the SDK's
/// own suite-acquire helper, and #704 recorded four AE effects answering it.
/// The rest of `A_Err` (1..=6, 22..=24) is what an AEGP suite call answers, not
/// a selector, and naming those here would label a plug-in's own code 2 as
/// `A_Err_STRUCT` and send a reader after a host failure that never happened.
#[doc(hidden)]
pub fn pf_error_name(code: i64) -> Option<&'static str> {
    Some(match code {
        4 => "PF_Err_OUT_OF_MEMORY",
        13 => "A_Err_MISSING_SUITE",
        512 => "PF_Err_INTERNAL_STRUCT_DAMAGED",
        513 => "PF_Err_INVALID_INDEX",
        514 => "PF_Err_UNRECOGNIZED_PARAM_TYPE",
        515 => "PF_Err_INVALID_CALLBACK",
        516 => "PF_Err_BAD_CALLBACK_PARAM",
        517 => "PF_Interrupt_CANCEL",
        518 => "PF_Err_CANNOT_PARSE_KEYFRAME_TEXT",
        _ => return None,
    })
}

/// How many identical troubles pass silently between reports. A frame that
/// keeps failing does so at the preview's frame rate, and one line per frame
/// would bury the log it is supposed to make readable.
const FRAME_TROUBLE_REPORT_INTERVAL: u32 = 60;

/// What was last reported for one filter, so a run of identical troubles
/// becomes one line plus a periodic count, and a recovery is reported once.
struct FrameTroubleState {
    summary: String,
    count: u32,
}

type FrameTroubleStates = HashMap<PathBuf, FrameTroubleState>;

static FRAME_TROUBLE: Mutex<Option<FrameTroubleStates>> = Mutex::new(None);

/// What to say about a frame that did not render, or nothing when this filter
/// is already known to be failing this way and the interval has not come round.
///
/// Kept apart from the logging so the rule - one line when it starts, one every
/// `FRAME_TROUBLE_REPORT_INTERVAL` after that - is testable without a host
/// logger or a global.
fn frame_trouble_report(
    states: &mut FrameTroubleStates,
    plugin: &Path,
    trouble: FrameTrouble<'_>,
) -> Option<String> {
    // The plug-in's own words ride on the lines that get emitted but stay out
    // of the identity the run is collapsed on. A plug-in is free to vary its
    // message per frame ("bad sample at t=..."), and folding that into the
    // identity would make every frame its own line at preview frame rate -
    // exactly the burial FRAME_TROUBLE_REPORT_INTERVAL exists to prevent.
    let said = match trouble {
        FrameTrouble::Error(_, said) => said,
        FrameTrouble::SessionLost(_) | FrameTrouble::Refused(_) => None,
    };
    let detail = said.map(|text| format!(": {text}")).unwrap_or_default();
    let summary = match trouble {
        FrameTrouble::Error(code, _) => match pf_error_name(code) {
            Some(name) => format!("frame error {code} ({name})"),
            None => format!("frame error {code}"),
        },
        FrameTrouble::SessionLost(reason) => format!("session lost: {reason}"),
        FrameTrouble::Refused(reason) => format!("frame refused: {reason}"),
    };
    match states.get_mut(plugin) {
        Some(state) if state.summary == summary => {
            state.count += 1;
            (state.count % FRAME_TROUBLE_REPORT_INTERVAL == 0).then(|| {
                format!(
                    "{}: {summary}{detail} (x{})",
                    filter_stem(plugin),
                    state.count
                )
            })
        }
        _ => {
            let line = format!("{}: {summary}{detail}", filter_stem(plugin));
            states.insert(
                plugin.to_path_buf(),
                FrameTroubleState { summary, count: 1 },
            );
            Some(line)
        }
    }
}

/// What to say when a filter renders again, or nothing when it was never
/// reported as failing - a healthy filter stays silent.
fn frame_recovered_report(states: &mut FrameTroubleStates, plugin: &Path) -> Option<String> {
    states.remove(plugin).map(|state| {
        format!(
            "{}: rendering again after {} frame(s) of {}",
            filter_stem(plugin),
            state.count,
            state.summary
        )
    })
}

/// Reports a frame that did not render.
///
/// Without this an effect erroring on every frame is indistinguishable from an
/// effect that does nothing: the pixels are left as they are either way, and
/// AviUtl2 shows the frame it already had. `Displacement` returning
/// `PF_Err_OUT_OF_MEMORY` on every frame (#695) was found only after a
/// throwaway build was made to print exactly this (issue #697).
fn report_frame_trouble(plugin: &Path, trouble: FrameTrouble<'_>) {
    let Ok(mut states) = FRAME_TROUBLE.lock() else {
        return;
    };
    if let Some(line) =
        frame_trouble_report(states.get_or_insert_with(HashMap::new), plugin, trouble)
    {
        log_warn(&line);
    }
}

/// Reports that a filter is rendering again after [`report_frame_trouble`] said
/// it was not.
fn report_frame_recovered(plugin: &Path) {
    let Ok(mut states) = FRAME_TROUBLE.lock() else {
        return;
    };
    let Some(states) = states.as_mut() else {
        return;
    };
    if let Some(line) = frame_recovered_report(states, plugin) {
        log_info(&line);
    }
}

/// Which session serves one frame (issue #405): the per-effect session (one
/// AEX, keyed by `effect_id`), or the pooled cluster session keyed by
/// (closure identity, geometry, smart) with the frame's plugin selected by
/// manifest index.
enum SessionRoute {
    PerEffect(Sender<RenderReq>, u64),
    Pooled(Sender<RenderReq>, u64, u32, PoolKey),
}

/// Selects the session for one frame. The pooled cluster session wins
/// whenever this AEX shares its closure identity with at least one other
/// registered AEX of the same smart flavor (design §8); singleton and
/// structurally oversized clusters keep the per-effect path (fail-closed).
fn route_session(
    ctx: &FilterCtx,
    effect_id: i64,
    identity: &GeomIdentity,
    // Called only when a session is actually opened, so an existing session's
    // reuse never pays for reading the host virtual buffer (issue #645).
    open_layers: &dyn Fn() -> Vec<SessionLayer>,
) -> Result<SessionRoute, ()> {
    // An AEX with a layer parameter stays on its per-effect session: only there
    // can the virtual buffer be supplied (a pooled session is shared by members
    // whose parameter layouts differ, so a layer slot valid for one member may
    // be a non-layer parameter in another, which the worker fails closed).
    //
    // Issue #816: discovery and render are both in-place, so a search-root
    // identity can always pool compatible members.
    let identity_pools = ctx.closure_identity.is_some();
    if ctx.layer_slots.is_empty()
        && identity_pools
        && let Some(closure_identity) = &ctx.closure_identity
    {
        let key = PoolKey {
            closure_identity: closure_identity.clone(),
            geom: identity.clone(),
            smart: ctx.smart,
        };
        if let Some((tx, serial, plugin_index)) = pool_sender(&key, &ctx.plugin) {
            return Ok(SessionRoute::Pooled(tx, serial, plugin_index, key));
        }
        let members = cluster_registry_members(&key, &ctx.plugin);
        if members.len() >= 2 && members.len() <= MAX_CLUSTER_PLUGINS {
            return pool_open_route(ctx, &key, members).map_err(|_| ());
        }
    }
    let (tx, serial) = match existing_sender(&ctx.sessions, effect_id, identity) {
        Some(pair) => pair,
        None => open_and_get_sender(ctx, effect_id, identity, open_layers).map_err(|_| ())?,
    };
    Ok(SessionRoute::PerEffect(tx, serial))
}

/// Opens a pooled cluster session for `key` off-lock, then installs it under
/// a brief lock. If another thread won the race, keeps the installed session
/// and drops ours off-lock. `members` is the manifest order, requester
/// first.
fn pool_open_route(
    ctx: &FilterCtx,
    key: &PoolKey,
    members: Vec<ClusterMember>,
) -> Result<SessionRoute, String> {
    let plugins: Vec<(PathBuf, String)> = members
        .iter()
        .map(|member| (member.plugin.clone(), member.sha.clone()))
        .collect();
    let plugin_index = plugins
        .iter()
        .position(|(path, _)| path == &ctx.plugin)
        .ok_or_else(|| "requester is not a cluster member".to_owned())?
        as u32;
    // A swap preserves the member's PARAMS_SETUP state. Untouched discovered
    // defaults are definitions, not host assignments; actual per-object
    // changes arrive with the subsequent frame request.
    let swap_payloads = vec![None; members.len()];
    let opened = open_mf_session(MfSessionConfig {
        repository: ctx.repository.clone(),
        plugin: ctx.plugin.clone(),
        dependency: ctx.dependency.clone(),
        sha: ctx.sha.clone(),
        smart: ctx.smart,
        identity: key.geom.clone(),
        // No virtual-buffer layer on a pooled cluster session: the members share
        // one dependency closure but not a parameter layout, so the opener's
        // layer slot may be a non-layer parameter in another member, which the
        // worker fails closed (-3) — a regression for members that rendered
        // fine before. A layer-fed AEX keeps its per-effect session (issue #645).
        layers: Vec::new(),
        cluster: Some(ClusterLaunch {
            plugins: plugins.clone(),
            swap_payloads,
        }),
    })?;
    let serial = opened.serial;
    let plugin_paths: Vec<PathBuf> = plugins.iter().map(|(path, _)| path.clone()).collect();
    let mut discard = None;
    let (tx, serial, plugin_index) = {
        let mut guard = SESSION_POOL
            .lock()
            .map_err(|_| "session pool poisoned".to_owned())?;
        let map = guard.get_or_insert_with(HashMap::new);
        let existing = map.get_mut(key).and_then(|entry| {
            let index = entry.plugins.iter().position(|path| path == &ctx.plugin)?;
            entry.session.last_used = Instant::now();
            entry
                .session
                .sender()
                .map(|tx| (tx, entry.session.serial, index as u32))
        });
        match existing {
            // Lost the race; keep the installed session, discard ours off-lock.
            Some(installed) => {
                discard = Some(PoolEntry {
                    session: opened,
                    plugins: plugin_paths,
                });
                installed
            }
            None => {
                let tx = opened
                    .sender()
                    .expect("a freshly opened session has a live sender");
                map.insert(
                    key.clone(),
                    PoolEntry {
                        session: opened,
                        plugins: plugin_paths,
                    },
                );
                (tx, serial, plugin_index)
            }
        }
    };
    drop(discard);
    Ok(SessionRoute::Pooled(tx, serial, plugin_index, key.clone()))
}

/// The owned launch config moved into a session's thread.
struct MfSessionConfig {
    repository: PathBuf,
    plugin: PathBuf,
    dependency: DependencyConfig,
    sha: String,
    smart: bool,
    identity: GeomIdentity,
    /// Secondary layers read once at open (issue #645): AviUtl2's virtual buffer
    /// feeding an AEX layer parameter. Read on the AviUtl2 callback thread (only
    /// there can the host texture be read) and moved here for the session thread.
    layers: Vec<SessionLayer>,
    /// Cluster launch (issue #405): when set, the session opens over the
    /// whole same-closure cluster and swaps plugins per request instead of
    /// serving a single AEX.
    cluster: Option<ClusterLaunch>,
}

/// The cluster a pooled session opens over (issue #405): the manifest order
/// (requester first) and each member's swap payload. `plugins[0]` is always
/// the requesting AEX, matching the base request's positional contract.
struct ClusterLaunch {
    plugins: Vec<(PathBuf, String)>,
    swap_payloads: Vec<Option<String>>,
}

/// Opens a session on its own thread, which owns the `!Send` `RenderSession` and
/// serves render requests until the channel closes or the session is lost.
fn open_mf_session(config: MfSessionConfig) -> Result<MfSession, String> {
    let identity = config.identity.clone();
    let (tx, rx) = channel::<RenderReq>();
    let (open_tx, open_rx) = channel::<Result<(), String>>();

    let join = std::thread::Builder::new()
        .name("aex-multifilter-session".into())
        .spawn(move || {
            // PARAMS_SETUP already installed the plug-in's defaults. Sending
            // the same values back as assignments is observably different
            // from an untouched effect in AE. Frames carry only values that
            // differ from these defaults.
            let baseline = None;
            // Whether this session actually opened a layer the frames may
            // rewrite (issue #674). Read here, before `config` is borrowed into
            // the open request, and used by the frame loop below to tell "there
            // is no map to update" from "the map no longer fits".
            let dynamic_layer_open = config.layers.iter().any(|layer| layer.dynamic);
            // Issue #816: render is in-place only. The worker resolves the
            // closure through the same search roots discovery inspected.
            let roots = search_roots_for(&config.plugin, &config.dependency.dirs);
            if roots.is_empty() {
                let _ = open_tx.send(Err("no dependency search roots resolved".to_owned()));
                return;
            }
            let dependencies = Vec::new();
            let dependency_search_dirs = roots.clone();
            let request = SessionOpenRequest {
                repository: &config.repository,
                plugin_path: &config.plugin,
                plugin_sha256: &config.sha,
                parameters: baseline,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                // The multifilter bridge renders video frames only; an audio
                // source would come from the host's audio graph, which it
                // does not read (issue #339).
                audio_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers: &config.layers,
                dependencies,
                dependency_search_dirs,
                width: config.identity.width,
                height: config.identity.height,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: config.identity.time_step,
                total_time: config.identity.total_time,
                time_scale: config.identity.time_scale,
                frame_deadline: Duration::from_millis(FRAME_DEADLINE_MS),
                smart: config.smart,
                gpu_backend: RenderGpuBackend::Auto,
                gpu_runtime_policy: None,
                payload_override: None,
                launch_environment: Default::default(),
            };
            // A pooled session opens over the whole same-closure cluster
            // (issue #405): staging, hashing, the ACL, and the closure's
            // LoadLibrary happen once for every member. A structurally
            // infeasible cluster degrades fail-closed to the plain
            // single-plugin session for the requester.
            let (mut session, cluster_plugin_count) = match &config.cluster {
                Some(cluster) => {
                    let mut plugins = Vec::with_capacity(cluster.plugins.len());
                    for (path, sha) in &cluster.plugins {
                        let Some(expected_sha256) = decode_sha256_hex(sha) else {
                            let _ = open_tx.send(Err(format!(
                                "cluster member sha256 is undecodable: {}",
                                path.display()
                            )));
                            return;
                        };
                        let expected_size = match std::fs::metadata(path) {
                            Ok(metadata) => metadata.len(),
                            Err(error) => {
                                let _ = open_tx.send(Err(format!(
                                    "cluster member is unreadable: {}: {error}",
                                    path.display()
                                )));
                                return;
                            }
                        };
                        plugins.push(ApprovedImageArtifact {
                            path: path.clone(),
                            expected_sha256,
                            expected_size,
                        });
                    }
                    // The in-place session (issue #751) declares no closure:
                    // its module bound is the recorded audit's enumeration
                    // capacity, and feasibility is the plugin count plus the
                    // admitted-directory bound the manifest validation
                    // enforces (search dirs and member parents, deduplicated)
                    // — an over-scattered cluster degrades to per-effect
                    // sessions instead of failing every open.
                    let admitted_dirs = {
                        let mut dirs: std::collections::HashSet<String> = roots
                            .iter()
                            .map(|root| root.to_string_lossy().to_lowercase())
                            .collect();
                        for (path, _) in &cluster.plugins {
                            if let Some(parent) = path.parent() {
                                dirs.insert(parent.to_string_lossy().to_lowercase());
                            }
                        }
                        dirs.len()
                    };
                    let module_bound = MAX_CLUSTER_MODULE_BOUND as u32;
                    let infeasible = plugins.len() > MAX_CLUSTER_PLUGINS
                        || admitted_dirs > MAX_CLUSTER_ADMITTED_DIRS;
                    if infeasible {
                        match RenderSession::open(request) {
                            Ok(session) => (session, 0),
                            Err(error) => {
                                let _ = open_tx
                                    .send(Err(format!("RenderSession::open failed: {error}")));
                                return;
                            }
                        }
                    } else {
                        match RenderSession::open_cluster(
                            request,
                            ClusterRenderPlugins {
                                plugins,
                                swap_payloads: cluster.swap_payloads.clone(),
                                module_bound,
                            },
                        ) {
                            Ok(session) => (session, cluster.plugins.len() as u32),
                            Err(error) => {
                                let _ = open_tx.send(Err(format!(
                                    "RenderSession::open_cluster failed: {error}"
                                )));
                                return;
                            }
                        }
                    }
                }
                None => match RenderSession::open(request) {
                    Ok(session) => (session, 0),
                    Err(error) => {
                        let _ = open_tx.send(Err(format!("RenderSession::open failed: {error}")));
                        return;
                    }
                },
            };
            if open_tx.send(Ok(())).is_err() {
                record_session_close(&config, &session.close());
                return;
            }

            // `frame_index` is a transport serial for the worker's non-advancing
            // check, decoupled from AviUtl2's `object.frame` (the host re-renders
            // and scrubs the same frame); AE time rides `current_time`.
            let mut frame_index: u32 = 0;
            let mut current_plugin: u32 = 0;
            while let Ok(req) = rx.recv() {
                // Pooled cluster session: swap to the frame's plugin first
                // (design §4.1). A plugin-local GLOBAL_SETUP failure is
                // reported frame-local and the session stays usable; an
                // invalidation loses the session (design §6).
                if cluster_plugin_count > 0 && req.plugin_index != current_plugin {
                    if req.plugin_index >= cluster_plugin_count {
                        let _ = req.reply.send(FrameReply::SessionLost(
                            "cluster plugin index is outside the manifest".into(),
                        ));
                        break;
                    }
                    match session.swap_plugin(req.plugin_index) {
                        Ok(SwapOutcome::Swapped) => current_plugin = req.plugin_index,
                        Ok(SwapOutcome::PluginError { global_setup_error }) => {
                            current_plugin = req.plugin_index;
                            let _ = req
                                .reply
                                .send(FrameReply::FrameLocal(global_setup_error, None));
                            continue;
                        }
                        Err(error) => {
                            let _ = req.reply.send(FrameReply::SessionLost(format!(
                                "swap_plugin failed: {error}"
                            )));
                            break;
                        }
                    }
                }
                // The map for this frame goes in before the frame does: the
                // worker re-reads the layer file when it renders, and this
                // thread is the only writer, so the request/response cycle is
                // what keeps a frame from seeing half an update (issue #674).
                // A rejected update fails the frame rather than rendering the
                // previous map under a new frame's parameters.
                //
                // A session that opened without a dynamic layer - the virtual
                // buffer had nothing in it when the session opened - has
                // nothing to update. Dropping it once per frame would reopen it
                // once per frame forever, so it simply renders without the map
                // until something else reopens it. A session that HAS the layer
                // and still rejects the update is the other case: the pixels no
                // longer fit what the worker was handed, so the session goes
                // and the next frame opens one at the new geometry.
                if let Some((slot, pixels)) = &req.layer
                    && dynamic_layer_open
                    && let Err(error) = session.update_dynamic_layer(*slot, pixels)
                {
                    let _ = req.reply.send(FrameReply::SessionLost(format!(
                        "dynamic layer update failed: {error}"
                    )));
                    break;
                }
                let outcome = session.render_frame_with_parameters(
                    frame_index,
                    req.current_time,
                    &req.rgba,
                    req.parameters.as_deref(),
                );
                frame_index = frame_index.wrapping_add(1);
                let reply = match outcome {
                    Ok(outcome) => match outcome.status {
                        FrameStatus::Rendered {
                            pixels,
                            width,
                            height,
                            origin_x,
                            origin_y,
                        } => FrameReply::Rendered(RenderedFrame {
                            pixels,
                            width,
                            height,
                            origin_x,
                            origin_y,
                        }),
                        FrameStatus::FrameError {
                            render_error,
                            return_message,
                            ..
                        } => FrameReply::FrameLocal(
                            render_error,
                            return_message.map(|message| message.text),
                        ),
                    },
                    Err(error) => FrameReply::SessionLost(format!("render_frame failed: {error}")),
                };
                // A host-protection invariant failure invalidates the whole
                // session; report it lost so the next frame reopens.
                let reply = if session.invalidation().is_some() {
                    FrameReply::SessionLost(match reply {
                        FrameReply::SessionLost(message) => message,
                        // The plug-in's own words survive the escalation: an
                        // invalidated session is where the reason matters most.
                        FrameReply::FrameLocal(code, said) => match said {
                            Some(text) => {
                                format!("session invalidated (render_error {code}: {text})")
                            }
                            None => format!("session invalidated (render_error {code})"),
                        },
                        FrameReply::Rendered(_) => "session invalidated".to_string(),
                    })
                } else {
                    reply
                };
                let lost = matches!(reply, FrameReply::SessionLost(_));
                let _ = req.reply.send(reply);
                if lost {
                    break;
                }
            }
            // The close-time checks (module audit, teardown, worker exit) are
            // part of the session's acceptance criteria (design §5/§7): a
            // non-clean close invalidates the session's delivered frames and
            // is recorded structurally, never dropped.
            record_session_close(&config, &session.close());
        })
        .map_err(|error| format!("failed to spawn session thread: {error}"))?;

    match open_rx.recv() {
        Ok(Ok(())) => Ok(MfSession {
            tx: Some(tx),
            identity,
            serial: SESSION_SERIAL.fetch_add(1, Ordering::Relaxed),
            last_used: Instant::now(),
            join: Some(join),
        }),
        Ok(Err(message)) => {
            let _ = join.join();
            Err(message)
        }
        Err(_) => {
            let _ = join.join();
            Err("session thread exited before reporting open result".into())
        }
    }
}

/// Renders one frame by round-tripping through a session's owning thread, with
/// no map lock held. `plugin_index` selects the frame's plugin inside a pooled
/// cluster session (0 for a single-plugin session, which never swaps).
fn render_on(
    tx: &Sender<RenderReq>,
    plugin_index: u32,
    current_time: i32,
    rgba: Vec<u8>,
    parameters: Option<Vec<InteractiveParameter>>,
    layer: Option<(u32, Vec<u8>)>,
) -> FrameReply {
    let (reply_tx, reply_rx) = channel();
    if tx
        .send(RenderReq {
            current_time,
            rgba,
            parameters,
            layer,
            plugin_index,
            reply: reply_tx,
        })
        .is_err()
    {
        return FrameReply::SessionLost("session thread is gone".to_string());
    }
    match reply_rx.recv() {
        Ok(reply) => reply,
        Err(_) => FrameReply::SessionLost("session thread dropped the reply".to_string()),
    }
}

type SessionMap = Mutex<HashMap<i64, MfSession>>;

/// Returns the sender + serial of a live session matching `identity`, refreshing
/// its `last_used`, or `None` to open one. A mismatched session (object resized/
/// retimed) is evicted; the eviction is dropped after the lock is released.
fn existing_sender(
    sessions: &SessionMap,
    effect_id: i64,
    identity: &GeomIdentity,
) -> Option<(Sender<RenderReq>, u64)> {
    let mut evicted: Option<MfSession> = None;
    let result;
    {
        let Ok(mut map) = sessions.lock() else {
            return None;
        };
        match map.get_mut(&effect_id) {
            Some(session) if &session.identity == identity => {
                session.last_used = Instant::now();
                result = session.sender().map(|tx| (tx, session.serial));
            }
            Some(_) => {
                evicted = map.remove(&effect_id);
                result = None;
            }
            None => result = None,
        }
    }
    drop(evicted);
    result
}

/// Opens a session off-lock, then installs it under a brief lock. If another
/// thread won the race, keeps that one and drops ours off-lock. Reaps idle
/// sessions opportunistically.
fn open_and_get_sender(
    ctx: &FilterCtx,
    effect_id: i64,
    identity: &GeomIdentity,
    open_layers: &dyn Fn() -> Vec<SessionLayer>,
) -> Result<(Sender<RenderReq>, u64), String> {
    let opened = open_mf_session(MfSessionConfig {
        repository: ctx.repository.clone(),
        plugin: ctx.plugin.clone(),
        dependency: ctx.dependency.clone(),
        sha: ctx.sha.clone(),
        smart: ctx.smart,
        identity: identity.clone(),
        layers: open_layers(),
        cluster: None,
    })?;
    let serial = opened.serial;
    let mut discard: Vec<MfSession> = Vec::new();
    let sender;
    {
        let mut map = ctx
            .sessions
            .lock()
            .map_err(|_| "session map poisoned".to_string())?;

        let now = Instant::now();
        let expired: Vec<i64> = map
            .iter()
            .filter(|(_, session)| now.duration_since(session.last_used) > SESSION_IDLE_TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(session) = map.remove(&id) {
                discard.push(session);
            }
        }

        match map
            .get(&effect_id)
            .filter(|session| &session.identity == identity)
            .and_then(|session| session.sender().map(|tx| (tx, session.serial)))
        {
            // Lost the race; keep the installed session, discard ours. No clone of
            // `opened`'s channel is taken here, so the off-lock drop/join cannot
            // wait on a stray sender that outlives it.
            Some(existing) => {
                discard.push(opened);
                sender = existing;
            }
            None => {
                let tx = opened
                    .sender()
                    .expect("a freshly opened session has a live sender");
                if let Some(old) = map.insert(effect_id, opened) {
                    discard.push(old);
                }
                sender = (tx, serial);
            }
        }
    }
    drop(discard);
    Ok(sender)
}

/// Removes and drops the session at `effect_id` matching `serial` (dropped
/// off-lock). Matching on serial avoids dropping a healthy session a concurrent
/// reopen installed at the same id.
fn remove_session(sessions: &SessionMap, effect_id: i64, serial: u64) {
    let removed = {
        let Ok(mut map) = sessions.lock() else {
            return;
        };
        if map
            .get(&effect_id)
            .is_some_and(|session| session.serial == serial)
        {
            map.remove(&effect_id)
        } else {
            None
        }
    };
    drop(removed);
}

/// Reads each item's current (keyframed) value into the matching parameter.
fn apply_readers(parameters: &mut [InteractiveParameter], readers: &[ItemReader]) {
    for reader in readers {
        match reader {
            ItemReader::Track { ptr, slot, integer } => {
                let value = unsafe { (**ptr).value };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    p.value = if *integer { value.round() } else { value };
                }
            }
            ItemReader::Checkbox { ptr, slot } => {
                let value = unsafe { (**ptr).value };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    p.value = if value { 1.0 } else { 0.0 };
                }
            }
            ItemReader::Select { ptr, slot } => {
                let value = unsafe { (**ptr).value };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    p.value = f64::from(value);
                }
            }
            ItemReader::Color { ptr, slot } => {
                let bgrx = unsafe { (**ptr).value.bgrx };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    // color is ARGB [a, r, g, b]; keep alpha, update rgb.
                    p.color[1] = bgrx[2];
                    p.color[2] = bgrx[1];
                    p.color[3] = bgrx[0];
                }
            }
        }
    }
}

/// Selects values whose typed payload differs from the PARAMS_SETUP default.
/// Metadata changes do not mutate a rendered parameter and are ignored.
fn changed_interactive_parameters(
    defaults: &[InteractiveParameter],
    current: &[InteractiveParameter],
) -> Vec<InteractiveParameter> {
    current
        .iter()
        .filter(|value| {
            let Some(default) = defaults.iter().find(|item| item.slot == value.slot) else {
                return true;
            };
            if default.kind != value.kind {
                return true;
            }
            match value.kind.as_str() {
                "integer" | "path" | "float" => default.value != value.value,
                "color" => default.color != value.color,
                "angle" => default.components[0] != value.components[0],
                "point" => default.components[..2] != value.components[..2],
                "point3d" => default.components != value.components,
                "arbitrary_data" => default.debug_summary != value.debug_summary,
                _ => false,
            }
        })
        .cloned()
        .collect()
}

fn pixels_to_bytes(pixels: &[PIXEL_RGBA]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for p in pixels {
        out.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    out
}

fn bytes_to_pixels(bytes: &[u8]) -> Vec<PIXEL_RGBA> {
    bytes
        .chunks_exact(4)
        .map(|c| PIXEL_RGBA {
            r: c[0],
            g: c[1],
            b: c[2],
            a: c[3],
        })
        .collect()
}
