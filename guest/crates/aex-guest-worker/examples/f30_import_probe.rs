fn main() {
    let args: Vec<String> = std::env::args().collect();
    aex_guest_worker::x64::f30_import_probe(&args[1], &args[2], args[3].parse().unwrap());
}
