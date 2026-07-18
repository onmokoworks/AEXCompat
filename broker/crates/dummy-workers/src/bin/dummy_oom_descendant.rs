// Spawns the given child (dummy_oom in tests), waits for it to die at the
// job memory cap, then exits nonzero itself. The parent worker never nears
// the cap, so its failure must not be classified as a memory-limit death.
fn main() {
    let child = std::env::args().nth(1).expect("child executable path");
    let status = std::process::Command::new(child)
        .status()
        .expect("spawn child");
    println!("child exited with {status}");
    std::process::exit(7);
}
