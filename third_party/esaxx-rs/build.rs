#[cfg(feature = "cpp")]
#[cfg(not(target_os = "macos"))]
fn main() {
    let mut build = cc::Build::new();
    build.cpp(true).flag("-std=c++11");
    #[cfg(target_os = "windows")]
    {
        build.static_crt(false);
    }
    build.file("src/esaxx.cpp").include("src").compile("esaxx");
}

#[cfg(feature = "cpp")]
#[cfg(target_os = "macos")]
fn main() {
    cc::Build::new()
        .cpp(true)
        .flag("-std=c++11")
        .flag("-stdlib=libc++")

        .file("src/esaxx.cpp")
        .include("src")
        .compile("esaxx");
}

#[cfg(not(feature = "cpp"))]
fn main() {}
