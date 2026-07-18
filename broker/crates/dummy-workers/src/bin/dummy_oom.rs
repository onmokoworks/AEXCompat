// Commits memory in small chunks until the Job Object cap rejects an
// allocation, then exits with a recognizable nonzero code. Used to test
// memory-limit kill-reason detection.
fn main() {
    const CHUNK: usize = 8 * 1024 * 1024;
    let mut held: Vec<Vec<u8>> = Vec::new();
    loop {
        let mut chunk: Vec<u8> = Vec::new();
        if chunk.try_reserve_exact(CHUNK).is_err() {
            println!("allocation failed after {} MiB", held.len() * 8);
            std::process::exit(42);
        }
        // Touch every page so the commit is real.
        chunk.resize(CHUNK, 0xAB);
        held.push(chunk);
    }
}
