fn main() {
    println!("cargo:rerun-if-env-changed=DOME_BUILD_DRM_DEVICE");
    if let Ok(device) = std::env::var("DOME_BUILD_DRM_DEVICE") {
        assert!(std::path::Path::new(&device).is_absolute(), "DOME_BUILD_DRM_DEVICE must be an absolute DRM device path");
        assert!(std::env::var_os("CARGO_FEATURE_MULTI_GPU").is_none(), "DOME_BUILD_DRM_DEVICE and multi-gpu are mutually exclusive");
    }
    println!("cargo:rerun-if-changed=native/qt_chrome.cpp");
    println!("cargo:rerun-if-changed=native/fltk_appearance.h");
    println!("cargo:rerun-if-changed=native/bdf_font.hpp");
    println!("cargo:rerun-if-changed=native/bdf_font_ffi.hpp");
    println!("cargo:rerun-if-changed=native/suite-icons.h");
    println!("cargo:rerun-if-changed=native/keyboard-flags.h");
    println!("cargo:rerun-if-changed=native/theme_icons.hpp");
    println!("cargo:rerun-if-changed=native/gpu-window.h");
    println!("cargo:rerun-if-changed=native/bitmap_fonts.hpp");
    println!("cargo:rerun-if-changed=native/default_apps.cpp");
    println!("cargo:rerun-if-changed=native/gtk_appearance.c");
    println!("cargo:rerun-if-env-changed=DOME_QT6_PREFIX");
    println!("cargo:rerun-if-env-changed=DOME_QT6_LIBDIR");
    if std::env::var_os("CARGO_FEATURE_NESTED").is_none() {
        return;
    }
    println!("cargo:rerun-if-changed=native/screencast.c");
    println!("cargo:rerun-if-changed=native/portal_picker.cpp");
    println!("cargo:rerun-if-changed=native/screen_share_status.h");
    let pipewire = pkg_config::Config::new().probe("libpipewire-0.3")
        .expect("PipeWire development files are required for screen sharing");
    let mut capture = cc::Build::new();
    for path in pipewire.include_paths { capture.include(path); }
    capture.file("native/screencast.c").flag("-std=gnu11").compile("dome_screencast");
    let gio = pkg_config::Config::new().cargo_metadata(false).probe("gio-2.0").expect("GIO development files are required");
    let module = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("libdome-appearance.so");
    let mut compiler = cc::Build::new().get_compiler().to_command();
    compiler.args(["-shared", "-fPIC", "-O2", "native/gtk_appearance.c", "-o"]).arg(module);
    for path in gio.include_paths { compiler.arg("-I").arg(path); }
    for path in gio.link_paths { compiler.arg("-L").arg(path); }
    for library in gio.libs { compiler.arg(format!("-l{library}")); }
    compiler.arg("-l:libgtk-3.so.0");
    assert!(compiler.status().expect("GTK appearance module compiler").success(), "GTK appearance module build failed");
    if std::env::var_os("CARGO_FEATURE_FLTK").is_some() {
        if std::env::var_os("CARGO_FEATURE_QT6").is_some() {
            panic!("Choose FLTK or Qt6; use --no-default-features for Qt6");
        }
        let prefix = std::env::var("DOME_FLTK_PREFIX").expect("Set DOME_FLTK_PREFIX to the isolated FLTK 1.4.5 install");
        println!("cargo:rerun-if-env-changed=DOME_FLTK_PREFIX");
        let mut native = cc::Build::new();
        native.cpp(true).std("c++17").include(format!("{prefix}/include"));
        for dependency in ["fontconfig", "freetype2", "gio-unix-2.0", "cairo", "zlib"] {
            let lib = pkg_config::Config::new().probe(dependency).expect("FLTK shell dependency");
            for path in lib.include_paths { native.include(path); }
        }
        for source in ["native/fltk_chrome.cpp", "native/fltk_default_apps.cpp", "native/fltk_portal_picker.cpp"] {
            println!("cargo:rerun-if-changed={source}");
            native.file(source);
        }
        println!("cargo:rustc-link-search=native={prefix}/lib");
        println!("cargo:rustc-link-lib=dylib=fltk");
        println!("cargo:rustc-link-lib=dylib=fltk_images");
        println!("cargo:rustc-link-lib=dylib=fltk_gl");
        println!("cargo:rustc-link-lib=dylib=GL");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{prefix}/lib");
        native.compile("dome_fltk_chrome");
        return;
    }
    let mut build = cc::Build::new();
    for dependency in ["fontconfig", "freetype2", "gio-unix-2.0", "zlib"] {
        let library = pkg_config::Config::new().probe(dependency)
            .expect("Fontconfig and FreeType development files are required for bitmap fonts");
        for path in library.include_paths { build.include(path); }
    }
    build
        .cpp(true)
        .std("c++17")
        .file("native/qt_chrome.cpp")
        .file("native/default_apps.cpp")
        .file("native/portal_picker.cpp")
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
