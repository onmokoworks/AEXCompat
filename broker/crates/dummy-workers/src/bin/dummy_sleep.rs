use std::time::Duration;

fn main() {
    let milliseconds = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30_000);
    std::thread::sleep(Duration::from_millis(milliseconds));
}
