fn main() {
    println!("cargo:rerun-if-changed=native/qt_chrome.cpp");
    println!("cargo:rerun-if-env-changed=DOME_QT6_PREFIX");
    println!("cargo:rerun-if-env-changed=DOME_QT6_LIBDIR");
    if std::env::var_os("CARGO_FEATURE_NESTED").is_none() {
        return;
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .file("native/qt_chrome.cpp")
        .flag("-fPIC");
    if let Some(prefix) = std::env::var_os("DOME_QT6_PREFIX") {
        // Optional extracted development headers; runtime libraries remain on the host.
        let prefix = std::path::Path::new(&prefix);
        // Fedora RPMs and Debian packages use different Qt header layouts.
        let fedora = prefix.join("include/qt6");
        build.include(if fedora.is_dir() {
            fedora
        } else {
            prefix.join("include/x86_64-linux-gnu/qt6")
        });
        let libdir = std::env::var("DOME_QT6_LIBDIR").unwrap_or("/usr/lib/x86_64-linux-gnu".into());
        for lib in ["Qt6Widgets", "Qt6Gui", "Qt6Core"] {
            println!("cargo:rustc-link-arg={libdir}/lib{lib}.so.6");
        }
        println!("cargo:rustc-link-arg={libdir}/libstdc++.so.6");
        build.cpp_link_stdlib(None);
    } else {
        let qt = pkg_config::Config::new()
            .atleast_version("6.4")
            .probe("Qt6Widgets")
            .expect("Qt6 Widgets development files are required for desktop chrome");
        for path in qt.include_paths {
            build.include(path);
        }
    }
    build.compile("dome_qt_chrome");
}
