use sha2::{Digest, Sha256};

fn hash_pixel(x: i32, y: i32, seed: i32, channel: i32) -> f32 {
    let mut value = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263))
        .wrapping_add((seed as u32).wrapping_mul(2_246_822_519))
        .wrapping_add((channel as u32).wrapping_mul(3_266_489_917));
    value ^= value >> 13;
    value = value.wrapping_mul(274_177);
    value ^= value >> 16;
    value = value.wrapping_mul(1_900_813);
    value ^= value >> 13;
    (value as f32 / u32::MAX as f32) * 2.0 - 1.0
}

pub fn argb8_hash(amount: i32, direction: i32, seed: i32, mix: f64) -> String {
    const WIDTH: i32 = 16;
    const HEIGHT: i32 = 12;
    let mut source = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let offset = ((y * WIDTH + x) * 4) as usize;
            source[offset] = 255;
            source[offset + 1] = (x * 255 / (WIDTH - 1)) as u8;
            source[offset + 2] = (y * 255 / (HEIGHT - 1)) as u8;
            source[offset + 3] = ((x + y) * 255 / (WIDTH + HEIGHT - 2)) as u8;
        }
    }
    let mut output = vec![0u8; source.len()];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dx = if direction == 1 || direction == 3 {
                (hash_pixel(x, y, seed, 0) * amount as f32).round() as i32
            } else {
                0
            };
            let dy = if direction == 2 || direction == 3 {
                (hash_pixel(x, y, seed, 1) * amount as f32).round() as i32
            } else {
                0
            };
            let source_x = (x + dx).clamp(0, WIDTH - 1);
            let source_y = (y + dy).clamp(0, HEIGHT - 1);
            let from = ((source_y * WIDTH + source_x) * 4) as usize;
            let to = ((y * WIDTH + x) * 4) as usize;
            output[to..to + 4].copy_from_slice(&source[from..from + 4]);
        }
    }
    if mix < 100.0 {
        let ratio = mix as f32 / 100.0f32;
        let inverse = 1.0f32 - ratio;
        for (result, original) in output.iter_mut().zip(source) {
            *result = ((original as f32 * inverse) + (*result as f32 * ratio)) as u8;
        }
    }
    format!("{:X}", Sha256::digest(output))
}

#[cfg(test)]
mod tests {
    use super::argb8_hash;

    #[test]
    fn matches_the_independent_python_oracle_matrix() {
        for (parameters, expected) in [
            (
                (5, 3, 0, 100.0),
                "19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9",
            ),
            (
                (0, 3, 0, 100.0),
                "863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7",
            ),
            (
                (9, 1, 17, 100.0),
                "82E72A2E7C05E831A45980FA4042940B8B84C4B6CCF020057968E88ACA323779",
            ),
            (
                (12, 3, 991, 37.5),
                "19736FAE645A7CD3BEBE865344E8A066E41040C7AB80042670A8B2EC6F0E1F3D",
            ),
            (
                (500, 3, 0, 100.0),
                "8E535435C74A9521D816A3B836DB578A2AE942EFBD80A55447B97610DC26B794",
            ),
            (
                (5, 3, 10_000, 100.0),
                "E31BA13264E801DE7CCCE4D6863215E54C0DC0C7FF4A918E45EE75BC59E817EC",
            ),
            (
                (500, 3, 10_000, 0.0),
                "863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7",
            ),
            (
                (13, 2, 1234, 33.333333333),
                "3905F287DBF3042CD73527154B8B6DA89E21A86C3ECDB6902D51A67F2CB79CF1",
            ),
        ] {
            assert_eq!(
                argb8_hash(parameters.0, parameters.1, parameters.2, parameters.3),
                expected
            );
        }
    }
}
