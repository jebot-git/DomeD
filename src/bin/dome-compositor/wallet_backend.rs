//! Runtime selection does not link a wallet toolkit into the compositor.
const NATIVE: &str = "/usr/local/libexec/domed/dome-walletd";
const PACKAGED_NATIVE: &str = "/usr/libexec/domed/dome-walletd";
const KWALLET: &str = "/usr/bin/ksecretd";

pub fn program(selection: &str, exists: impl Fn(&str) -> bool) -> Option<&'static str> {
    match selection {
        "native" => [NATIVE, PACKAGED_NATIVE]
            .into_iter()
            .find(|path| exists(path)),
        "kwallet" => exists(KWALLET).then_some(KWALLET),
        "auto" => [NATIVE, PACKAGED_NATIVE, KWALLET]
            .into_iter()
            .find(|path| exists(path)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_native_backend_is_preferred() {
        assert_eq!(program("auto", |_| true), Some(NATIVE));
    }
    #[test]
    fn packaged_native_backend_is_preferred_over_legacy_launcher() {
        assert_eq!(
            program("auto", |path| path != NATIVE),
            Some(PACKAGED_NATIVE)
        );
        assert_eq!(
            program("native", |path| path == PACKAGED_NATIVE),
            Some(PACKAGED_NATIVE)
        );
    }
    #[test]
    fn existing_kwallet_remains_a_compatibility_fallback() {
        assert_eq!(program("auto", |path| path == KWALLET), Some(KWALLET));
    }
    #[test]
    fn explicit_native_never_falls_back_to_qt() {
        assert_eq!(program("native", |path| path == KWALLET), None);
    }
    #[test]
    fn explicit_legacy_selection_is_retained() {
        assert_eq!(program("kwallet", |_| true), Some(KWALLET));
    }
    #[test]
    fn missing_disabled_and_unknown_backends_do_not_start_a_provider() {
        assert_eq!(program("auto", |_| false), None);
        assert_eq!(program("none", |_| true), None);
        assert_eq!(program("unknown", |_| true), None);
    }
}
