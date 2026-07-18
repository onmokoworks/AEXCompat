use std::time::Duration;

fn main() {
    if std::env::args().any(|arg| arg == "--child") {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }

    let executable = std::env::current_exe().expect("current executable");
    std::process::Command::new(executable)
        .arg("--child")
        .spawn()
        .expect("spawn pipe-holding descendant");
    println!("parent completed");
}
