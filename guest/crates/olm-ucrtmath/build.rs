fn main() {
    let mut build = cc::Build::new();
    build.include("ucrtmath/include").flag_if_supported("-std=c11")
        .flag_if_supported("-ffp-contract=off").flag_if_supported("-fno-fast-math")
        .flag_if_supported("/fp:strict");
    for name in ["expf", "atan2f", "powf", "tables"] {
        build.file(format!("ucrtmath/src/{name}.c"));
    }
    build.file("ucrtmath/ms/float.c");
    build.compile("ucrtmath");
    println!("cargo:rerun-if-changed=ucrtmath");
}
