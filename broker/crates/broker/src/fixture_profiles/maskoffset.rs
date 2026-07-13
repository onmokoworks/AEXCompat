use sha2::{Digest, Sha256};

pub const PROFILE_ID: &str = "maskoffset";

pub fn rectangle_mask_argb8_hash() -> String {
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
            if !(4..12).contains(&x) || !(3..9).contains(&y) {
                source[offset] = 0;
            }
        }
    }
    format!("{:X}", Sha256::digest(source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_mask_oracle_is_fixed() {
        assert_eq!(
            rectangle_mask_argb8_hash(),
            "D1003EF35A6EA3B037989F00672996FEA59867FBB50283A689AC95EDD9BF2359"
        );
    }
}
