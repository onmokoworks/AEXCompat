use std::io::{self, Write};

fn main() -> io::Result<()> {
    let bytes = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "byte count required"))?;
    let chunk = [b'x'; 64 * 1024];
    let mut remaining = bytes;
    let mut stdout = io::stdout().lock();
    while remaining != 0 {
        let take = remaining.min(chunk.len());
        stdout.write_all(&chunk[..take])?;
        remaining -= take;
    }
    Ok(())
}
