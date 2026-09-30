mod visual_diagnostics {
    use super::RenderPixelFormat;
    use serde_json::{Value, json};

    // This is deliberately an advisory, sampled view of native pixels. The
    // fixed grid limits work and report size even for a very large plug-in
    // world; exact input equality is offered only within a separate budget.
    const AXIS_LIMIT: usize = 128;
    const EXACT_PIXEL_LIMIT: usize = 262_144;

    #[derive(Clone, Copy)]
    struct Stats {
        count: u64,
        nonfinite: u64,
        zero: u64,
        maxed: u64,
        over_white: u64,
        min: f64,
        max: f64,
        sum: f64,
        sum_sq: f64,
        histogram: [u64; 8],
    }

    impl Default for Stats {
        fn default() -> Self {
            Self {
                count: 0,
                nonfinite: 0,
                zero: 0,
                maxed: 0,
                over_white: 0,
                min: f64::INFINITY,
                max: f64::NEG_INFINITY,
                sum: 0.0,
                sum_sq: 0.0,
                histogram: [0; 8],
            }
        }
    }

    impl Stats {
        fn add(&mut self, value: f64) {
            if !value.is_finite() {
                self.nonfinite += 1;
                return;
            }
            self.count += 1;
            self.min = self.min.min(value);
            self.max = self.max.max(value);
            self.sum += value;
            self.sum_sq += value * value;
            if value == 0.0 {
                self.zero += 1;
            }
            if value == 1.0 {
                self.maxed += 1;
            }
            if value > 1.0 {
                self.over_white += 1;
            }
            self.histogram[((value.clamp(0.0, 1.0) * 8.0) as usize).min(7)] += 1;
        }

        fn mean(self) -> f64 {
            if self.count == 0 {
                0.0
            } else {
                self.sum / self.count as f64
            }
        }
        fn variance(self) -> f64 {
            if self.count == 0 {
                0.0
            } else {
                (self.sum_sq / self.count as f64 - self.mean().powi(2)).max(0.0)
            }
        }
        fn report(self) -> Value {
            json!({"min": (self.count > 0).then_some(self.min),
                "max": (self.count > 0).then_some(self.max),
                "mean": (self.count > 0).then_some(self.mean()),
                "variance": (self.count > 0).then_some(self.variance()),
                "finite_count": self.count, "nonfinite_count": self.nonfinite,
                "zero_rate": rate(self.zero, self.count),
                "nominal_white_rate": rate(self.maxed, self.count),
                "over_white_rate": rate(self.over_white, self.count),
                "histogram_8": self.histogram})
        }
    }

    #[derive(Clone, Copy, Default)]
    struct ColorStats {
        luminance: Stats,
        rgb: [Stats; 3],
    }

    impl ColorStats {
        fn add(&mut self, rgb: [f64; 3]) {
            self.luminance.add((rgb[0] + rgb[1] + rgb[2]) / 3.0);
            for (stats, value) in self.rgb.iter_mut().zip(rgb) {
                stats.add(value);
            }
        }

        fn count(self) -> u64 {
            self.luminance.count
        }

        fn variance(self) -> f64 {
            self.rgb.iter().map(|stats| stats.variance()).sum::<f64>() / 3.0
        }

        fn mean_rgb(self) -> [f64; 3] {
            self.rgb.map(Stats::mean)
        }

        fn report(self) -> Value {
            json!({"luminance": self.luminance.report(),
                "rgb_mean": self.mean_rgb(), "rgb_variance": self.variance()})
        }
    }

    fn rgb_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.into_iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .sum::<f64>()
            / 3.0
    }

    fn rate(count: u64, total: u64) -> Option<f64> {
        (total > 0).then(|| count as f64 / total as f64)
    }

    fn axis_positions(length: u32) -> Vec<u32> {
        if length == 0 {
            return Vec::new();
        }
        let mut positions = Vec::with_capacity(AXIS_LIMIT + 5);
        for i in 0..AXIS_LIMIT.min(length as usize) {
            positions.push(
                ((i as u64 * (length - 1) as u64)
                    / (AXIS_LIMIT.min(length as usize) - 1).max(1) as u64) as u32,
            );
        }
        for center in [length / 2, length / 4, length.saturating_mul(3) / 4] {
            for neighbor in [
                center.saturating_sub(1),
                center,
                center.saturating_add(1).min(length - 1),
            ] {
                positions.push(neighbor);
            }
        }
        positions.sort_unstable();
        positions.dedup();
        positions
    }

    fn pixel(bytes: &[u8], index: usize, format: RenderPixelFormat) -> Option<[f64; 4]> {
        let stride = format.bytes_per_pixel() as usize;
        let start = index.checked_mul(stride)?;
        let data = bytes.get(start..start.checked_add(stride)?)?;
        let mut channels = [0.0; 4];
        for (channel, value) in channels.iter_mut().enumerate() {
            *value = match format {
                RenderPixelFormat::Argb8 => data[channel] as f64 / 255.0,
                RenderPixelFormat::Argb16 => {
                    u16::from_le_bytes([data[channel * 2], data[channel * 2 + 1]]) as f64 / 32768.0
                }
                RenderPixelFormat::Argb32f => {
                    f32::from_le_bytes(data[channel * 4..channel * 4 + 4].try_into().ok()?) as f64
                }
            };
        }
        Some(channels)
    }

    fn expected_input_pixel(
        input: &[u8],
        index: usize,
        format: RenderPixelFormat,
    ) -> Option<[u8; 16]> {
        let source = input.get(index.checked_mul(4)?..index.checked_mul(4)?.checked_add(4)?)?;
        let mut expected = [0u8; 16];
        for (channel_index, channel) in source.iter().enumerate() {
            match format {
                RenderPixelFormat::Argb8 => expected[channel_index] = *channel,
                RenderPixelFormat::Argb16 => expected[channel_index * 2..channel_index * 2 + 2]
                    .copy_from_slice(
                        &(((*channel as u32 * 32768 + 127) / 255) as u16).to_le_bytes(),
                    ),
                RenderPixelFormat::Argb32f => expected[channel_index * 4..channel_index * 4 + 4]
                    .copy_from_slice(&(*channel as f32 / 255.0).to_le_bytes()),
            }
        }
        Some(expected)
    }

    fn changed(
        input: &[u8],
        output: &[u8],
        index: usize,
        format: RenderPixelFormat,
    ) -> Option<bool> {
        let stride = format.bytes_per_pixel() as usize;
        let start = index.checked_mul(stride)?;
        Some(
            output.get(start..start.checked_add(stride)?)?
                != &expected_input_pixel(input, index, format)?[..stride],
        )
    }

    fn longest_constant_run(lines: &[ColorStats]) -> usize {
        let mut longest = 0;
        let mut current = 0;
        for stats in lines {
            if stats.count() > 0 && stats.variance() < 1e-6 {
                current += 1;
                longest = longest.max(current);
            } else {
                current = 0;
            }
        }
        longest
    }

    fn stripe_candidate(axes: &[u32], lines: &[ColorStats]) -> Option<(u32, f64)> {
        let mut best = None;
        for i in 1..axes.len().saturating_sub(1) {
            if axes[i - 1] + 1 != axes[i] || axes[i] + 1 != axes[i + 1] {
                continue;
            }
            let (a, b, c) = (lines[i - 1], lines[i], lines[i + 1]);
            if a.count() == 0 || b.count() == 0 || c.count() == 0 || b.variance() > 0.02 {
                continue;
            }
            let (left, middle, right) = (a.mean_rgb(), b.mean_rgb(), c.mean_rgb());
            let score = rgb_distance(
                middle,
                std::array::from_fn(|channel| (left[channel] + right[channel]) / 2.0),
            );
            if score > 0.08
                && rgb_distance(left, right) < 0.05
                && best.is_none_or(|(_, prior)| score > prior)
            {
                best = Some((axes[i], score));
            }
        }
        best
    }

    fn boundary_score(axes: &[u32], lines: &[ColorStats], boundary: u32) -> Option<f64> {
        let before = axes.binary_search(&boundary.checked_sub(1)?).ok()?;
        let after = axes.binary_search(&boundary).ok()?;
        Some(rgb_distance(
            lines[before].mean_rgb(),
            lines[after].mean_rgb(),
        ))
    }

    fn strongest_boundary(axes: &[u32], lines: &[ColorStats]) -> Option<(u32, f64)> {
        axes.windows(2)
            .enumerate()
            .filter(|(_, pair)| pair[0] + 1 == pair[1])
            .map(|(index, pair)| {
                (
                    pair[1],
                    rgb_distance(lines[index].mean_rgb(), lines[index + 1].mean_rgb()),
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    pub(super) fn inspect(
        input_rgba8: Option<&[u8]>,
        input_dims: (u32, u32),
        output: &[u8],
        output_dims: (u32, u32),
        format: RenderPixelFormat,
    ) -> Value {
        let (width, height) = output_dims;
        let Some(pixel_count) = (width as usize).checked_mul(height as usize) else {
            return json!({"advisory": true, "status": "unavailable", "reason": "invalid_dimensions"});
        };
        let Some(byte_count) = pixel_count.checked_mul(format.bytes_per_pixel() as usize) else {
            return json!({"advisory": true, "status": "unavailable", "reason": "invalid_dimensions"});
        };
        if pixel_count == 0 || output.len() != byte_count {
            return json!({"advisory": true, "status": "unavailable",
                "reason": if pixel_count == 0 { "empty_output" } else { "invalid_native_buffer" }});
        }
        let xs = axis_positions(width);
        let ys = axis_positions(height);
        let mut channels = [Stats::default(); 4];
        let mut halves = [ColorStats::default(); 4]; // left, right, top, bottom
        let mut quadrants = [ColorStats::default(); 4];
        let mut rows = vec![ColorStats::default(); ys.len()];
        let mut cols = vec![ColorStats::default(); xs.len()];
        let mut transparent = 0u64;
        let mut transparent_with_rgb = 0u64;
        let mut sample_changed = 0u64;
        let comparison_reason = match input_rgba8 {
            None => Some("input_unavailable"),
            Some(_) if input_dims != output_dims => Some("geometry_mismatch"),
            Some(input) if input.len() != pixel_count.saturating_mul(4) => {
                Some("invalid_input_buffer")
            }
            Some(_) => None,
        };
        for (iy, &y) in ys.iter().enumerate() {
            for (ix, &x) in xs.iter().enumerate() {
                let index = y as usize * width as usize + x as usize;
                let p = pixel(output, index, format).expect("validated native buffer");
                for (stats, value) in channels.iter_mut().zip(p) {
                    stats.add(value);
                }
                let rgb = [p[0], p[1], p[2]];
                if x < width / 2 {
                    halves[0].add(rgb);
                } else {
                    halves[1].add(rgb);
                }
                if y < height / 2 {
                    halves[2].add(rgb);
                } else {
                    halves[3].add(rgb);
                }
                quadrants[(y >= height / 2) as usize * 2 + (x >= width / 2) as usize].add(rgb);
                rows[iy].add(rgb);
                cols[ix].add(rgb);
                if p[3] == 0.0 {
                    transparent += 1;
                    if p[..3].iter().any(|v| *v != 0.0) {
                        transparent_with_rgb += 1;
                    }
                }
                if comparison_reason.is_none()
                    && changed(
                        input_rgba8.expect("comparison input"),
                        output,
                        index,
                        format,
                    ) == Some(true)
                {
                    sample_changed += 1;
                }
            }
        }
        let sample_count = (xs.len() * ys.len()) as u64;
        let mut exact_equal = None;
        let mut changed_rate = None;
        if comparison_reason.is_none() && pixel_count <= EXACT_PIXEL_LIMIT {
            let input = input_rgba8.expect("comparison input");
            let changed_count = (0..pixel_count)
                .filter(|&index| changed(input, output, index, format) == Some(true))
                .count();
            exact_equal = Some(changed_count == 0);
            changed_rate = rate(changed_count as u64, pixel_count as u64);
        }
        let vertical_stripe = stripe_candidate(&xs, &cols);
        let horizontal_stripe = stripe_candidate(&ys, &rows);
        // These deliberately conservative heuristics describe suspicious
        // structure, not a render contract. Legal effects (including an
        // intentional split or passthrough) may receive a reason code.
        let mut reasons = Vec::new();
        let mut candidate_bboxes = Vec::new();
        if exact_equal == Some(true) {
            reasons.push("unchanged_from_input");
        }
        if channels[..3].iter().all(|c| c.count > 0 && c.max == 0.0) {
            reasons.push("rgb_all_zero");
        }
        if channels[..3].iter().any(|c| c.count > 0 && c.max == 0.0)
            && channels[..3].iter().any(|c| c.max > 0.05)
        {
            reasons.push("channel_missing");
        }
        if channels[..3]
            .iter()
            .all(|c| c.count > 0 && c.zero + c.maxed == c.count)
            && channels[..3].iter().any(|c| c.maxed > 0)
        {
            reasons.push("full_clip");
        }
        for (constant, variable, bbox) in [
            (halves[0], halves[1], [0, 0, width / 2, height]),
            (halves[1], halves[0], [width / 2, 0, width, height]),
            (halves[2], halves[3], [0, 0, width, height / 2]),
            (halves[3], halves[2], [0, height / 2, width, height]),
        ] {
            if constant.count() > 0 && constant.variance() < 1e-6 && variable.variance() > 1e-4 {
                candidate_bboxes.push(json!({"reason": "one_sided_constant_region", "bbox": bbox}));
            }
        }
        if !candidate_bboxes.is_empty() {
            reasons.push("one_sided_constant_region");
        }
        if let Some((x, _)) = vertical_stripe {
            candidate_bboxes
                .push(json!({"reason": "axis_aligned_seam", "bbox": [x, 0, x + 1, height]}));
        }
        if let Some((y, _)) = horizontal_stripe {
            candidate_bboxes
                .push(json!({"reason": "axis_aligned_seam", "bbox": [0, y, width, y + 1]}));
        }
        if vertical_stripe.is_some() || horizontal_stripe.is_some() {
            reasons.push("axis_aligned_seam");
        }
        json!({
            "advisory": true, "status": "available", "pixel_format": format.report_name(),
            "sampled": xs.len() != width as usize || ys.len() != height as usize,
            "sample_count": sample_count, "sample_axis_limit": AXIS_LIMIT,
            "exact_comparison_pixel_limit": EXACT_PIXEL_LIMIT,
            "channels": {"r": channels[0].report(), "g": channels[1].report(),
                "b": channels[2].report(), "a": channels[3].report()},
            "transparent_rate": rate(transparent, sample_count),
            "transparent_with_rgb_rate": rate(transparent_with_rgb, sample_count),
            "regions": {"left": halves[0].report(), "right": halves[1].report(),
                "top": halves[2].report(), "bottom": halves[3].report(),
                "quadrants": quadrants.map(ColorStats::report)},
            "region_differences": {"left_right_rgb_mean": rgb_distance(halves[0].mean_rgb(), halves[1].mean_rgb()),
                "top_bottom_rgb_mean": rgb_distance(halves[2].mean_rgb(), halves[3].mean_rgb())},
            "candidate_bboxes": candidate_bboxes,
            "runs": {"constant_sampled_rows": longest_constant_run(&rows),
                "constant_sampled_columns": longest_constant_run(&cols)},
            "seam_candidates": {"vertical": vertical_stripe.map(|(coordinate, score)| json!({"x": coordinate, "score": score})),
                "horizontal": horizontal_stripe.map(|(coordinate, score)| json!({"y": coordinate, "score": score}))},
            "seam_scores": {"center_vertical": boundary_score(&xs, &cols, width / 2),
                "center_horizontal": boundary_score(&ys, &rows, height / 2),
                "strongest_vertical": strongest_boundary(&xs, &cols).map(|(x, score)| json!({"x": x, "score": score})),
                "strongest_horizontal": strongest_boundary(&ys, &rows).map(|(y, score)| json!({"y": y, "score": score}))},
            "comparison": {"status": if comparison_reason.is_some() { "unavailable" } else { "available" },
                "reason": comparison_reason,
                "exact_status": if exact_equal.is_some() { "available" } else { "unavailable" },
                "exact_unavailable_reason": if comparison_reason.is_some() { comparison_reason }
                    else if exact_equal.is_none() { Some("pixel_limit") } else { None },
                "exact_equal": exact_equal,
                "changed_pixel_ratio": changed_rate,
                "sampled_changed_pixel_ratio": if comparison_reason.is_none() { rate(sample_changed, sample_count) } else { None }},
            "reasons": reasons,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn render_case(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
            let mut out = Vec::new();
            for y in 0..height {
                for x in 0..width {
                    out.extend_from_slice(&pixel(x, y));
                }
            }
            out
        }

        #[test]
        fn suspicious_patterns_are_advisory_and_normal_controls_are_not_failures() {
            let input = render_case(32, 24, |x, y| [x as u8 * 4, y as u8 * 7, 80, 255]);
            let half_black = render_case(32, 24, |x, y| {
                if x < 16 {
                    [0, 0, 0, 255]
                } else {
                    [x as u8 * 4, y as u8 * 7, 80, 255]
                }
            });
            let diagnosis = inspect(
                Some(&input),
                (32, 24),
                &half_black,
                (32, 24),
                RenderPixelFormat::Argb8,
            );
            assert_eq!(diagnosis["advisory"], true);
            assert_eq!(diagnosis["status"], "available");
            assert!(
                diagnosis["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == "one_sided_constant_region")
            );

            let dark_control = render_case(32, 24, |x, y| {
                if (12..20).contains(&x) && (8..16).contains(&y) {
                    [48, 48, 48, 255]
                } else {
                    [4, 4, 4, 255]
                }
            });
            let control = inspect(
                Some(&input),
                (32, 24),
                &dark_control,
                (32, 24),
                RenderPixelFormat::Argb8,
            );
            assert_eq!(control["status"], "available");
            assert!(
                !control["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == "rgb_all_zero")
            );
        }

        #[test]
        fn native_depth_comparison_and_hdr_are_not_preview_clamped() {
            let input = vec![10, 20, 30, 255];
            let mut normalized_red = Vec::new();
            for (format, pixel) in [
                (RenderPixelFormat::Argb8, input.clone()),
                (
                    RenderPixelFormat::Argb16,
                    [10u8, 20, 30, 255]
                        .iter()
                        .flat_map(|v| (((*v as u32 * 32768 + 127) / 255) as u16).to_le_bytes())
                        .collect(),
                ),
                (
                    RenderPixelFormat::Argb32f,
                    [10u8, 20, 30, 255]
                        .iter()
                        .flat_map(|v| (*v as f32 / 255.0).to_le_bytes())
                        .collect(),
                ),
            ] {
                let diagnosis = inspect(Some(&input), (1, 1), &pixel, (1, 1), format);
                assert_eq!(diagnosis["comparison"]["exact_equal"], true);
                normalized_red.push(diagnosis["channels"]["r"]["mean"].as_f64().unwrap());
            }
            assert!(
                normalized_red
                    .iter()
                    .all(|value| (value - normalized_red[0]).abs() < 0.0001)
            );
            let hdr: Vec<u8> = [2.0f32, 0.5, 0.0, 1.0]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            let diagnosis = inspect(
                Some(&input),
                (1, 1),
                &hdr,
                (1, 1),
                RenderPixelFormat::Argb32f,
            );
            assert_eq!(diagnosis["channels"]["r"]["max"], 2.0);
            assert_eq!(diagnosis["channels"]["r"]["over_white_rate"], 1.0);
            assert!(
                !diagnosis["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == "full_clip")
            );
        }

        #[test]
        fn synthetic_failure_matrix_and_negative_controls() {
            let input = render_case(64, 48, |x, y| [20 + x as u8, 30 + y as u8, 50, 255]);
            let cases: Vec<(&str, Vec<u8>, &str)> = vec![
                ("unchanged", input.clone(), "unchanged_from_input"),
                (
                    "black_with_alpha",
                    render_case(64, 48, |_, _| [0, 0, 0, 255]),
                    "rgb_all_zero",
                ),
                (
                    "missing_green",
                    render_case(64, 48, |x, y| [x as u8 * 3, 0, y as u8 * 3, 255]),
                    "channel_missing",
                ),
                (
                    "top_constant",
                    render_case(64, 48, |x, y| {
                        if y < 24 {
                            [30, 30, 30, 255]
                        } else {
                            [x as u8 * 3, y as u8 * 3, 30, 255]
                        }
                    }),
                    "one_sided_constant_region",
                ),
                (
                    "one_pixel_vertical_seam",
                    render_case(64, 48, |x, _| {
                        if x == 32 {
                            [255, 255, 255, 255]
                        } else {
                            [20, 20, 20, 255]
                        }
                    }),
                    "axis_aligned_seam",
                ),
                (
                    "one_pixel_horizontal_seam",
                    render_case(64, 48, |_, y| {
                        if y == 24 {
                            [255, 255, 255, 255]
                        } else {
                            [20, 20, 20, 255]
                        }
                    }),
                    "axis_aligned_seam",
                ),
                (
                    "full_clip",
                    render_case(64, 48, |x, _| {
                        if x < 32 {
                            [0, 0, 0, 0]
                        } else {
                            [255, 255, 255, 255]
                        }
                    }),
                    "full_clip",
                ),
            ];
            for (name, output, reason) in cases {
                let diagnosis = inspect(
                    Some(&input),
                    (64, 48),
                    &output,
                    (64, 48),
                    RenderPixelFormat::Argb8,
                );
                assert!(
                    diagnosis["reasons"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|value| value == reason),
                    "{name}: {diagnosis}"
                );
            }
            let transparent_color = render_case(64, 48, |_, _| [100, 20, 10, 0]);
            let diagnosis = inspect(
                Some(&input),
                (64, 48),
                &transparent_color,
                (64, 48),
                RenderPixelFormat::Argb8,
            );
            assert_eq!(diagnosis["transparent_with_rgb_rate"], 1.0);
            let intentional_bisection = render_case(64, 48, |x, _| {
                if x < 32 {
                    [30, 30, 30, 255]
                } else {
                    [70, 70, 70, 255]
                }
            });
            let diagnosis = inspect(
                Some(&input),
                (64, 48),
                &intentional_bisection,
                (64, 48),
                RenderPixelFormat::Argb8,
            );
            assert!(
                !diagnosis["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == "axis_aligned_seam")
            );
            assert!(
                diagnosis["seam_scores"]["center_vertical"]
                    .as_f64()
                    .unwrap()
                    > 0.1
            );

            // Equal-luminance hue changes must not look like constant RGB.
            let isoluminant = render_case(64, 48, |x, _| {
                if x < 32 {
                    [80, 80, 80, 255]
                } else if x % 2 == 0 {
                    [240, 0, 0, 255]
                } else {
                    [0, 240, 0, 255]
                }
            });
            let diagnosis = inspect(
                Some(&input),
                (64, 48),
                &isoluminant,
                (64, 48),
                RenderPixelFormat::Argb8,
            );
            assert!(
                diagnosis["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == "one_sided_constant_region")
            );
            assert_eq!(diagnosis["runs"]["constant_sampled_rows"], 0);
        }

        #[test]
        fn comparison_unavailable_and_sampling_are_explicit_and_bounded() {
            let input = vec![0; 4];
            let output = vec![0; 4];
            let geometry = inspect(
                Some(&input),
                (1, 1),
                &output,
                (1, 2),
                RenderPixelFormat::Argb8,
            );
            assert_eq!(geometry["status"], "unavailable");
            let geometry = inspect(
                Some(&input),
                (2, 1),
                &output,
                (1, 1),
                RenderPixelFormat::Argb8,
            );
            assert_eq!(geometry["comparison"]["reason"], "geometry_mismatch");
            assert!(geometry["comparison"]["exact_equal"].is_null());
            let large = vec![0; 1024 * 1024 * 4];
            let diagnosis = inspect(
                Some(&large),
                (1024, 1024),
                &large,
                (1024, 1024),
                RenderPixelFormat::Argb8,
            );
            assert_eq!(diagnosis["sampled"], true);
            assert!(diagnosis["sample_count"].as_u64().unwrap() <= 137 * 137);
            assert_eq!(diagnosis["comparison"]["status"], "available");
            assert!(diagnosis["comparison"]["exact_equal"].is_null());
            assert_eq!(
                diagnosis["comparison"]["exact_unavailable_reason"],
                "pixel_limit"
            );
            assert_eq!(diagnosis["comparison"]["sampled_changed_pixel_ratio"], 0.0);
            assert!(serde_json::to_vec(&diagnosis).unwrap().len() < 5000);
            let no_input = inspect(None, (1, 1), &output, (1, 1), RenderPixelFormat::Argb8);
            assert_eq!(no_input["comparison"]["reason"], "input_unavailable");
        }

        #[test]
        fn large_frame_diagnostics_finish_within_a_fixed_budget() {
            use std::time::{Duration, Instant};

            let output = vec![0u8; 4096 * 4096 * 4];
            let started = Instant::now();
            let report = inspect(
                None,
                (4096, 4096),
                &output,
                (4096, 4096),
                RenderPixelFormat::Argb8,
            );
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "bounded pixel sampling exceeded five seconds"
            );
            assert!(report["sample_count"].as_u64().unwrap() <= 137 * 137);
            assert!(serde_json::to_vec(&report).unwrap().len() < 5000);
        }
    }
}
