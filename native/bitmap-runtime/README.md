DomeD native bitmap runtime
==========================

The default runtime accepts decoded, nonscalable BDF and PCF fonts. FreeType
checks the decoded face at open time; HarfBuzz shapes the native Unicode strike;
Cairo validates external FreeType faces; Pango selects bitmap-only Fontconfig
matches and preserves strike metrics. Renaming an outline file does not change
its decoded format. Native PCF, compressed PCF, and BDF use the same pipeline.

Build as the regular user:

    python3 scripts/prepare-bitmap-runtime.py --prefix build/bitmap-runtime --jobs 8
    python3 scripts/test-bitmap-runtime.py --prefix build/bitmap-runtime --pcf /path/to/unicode.pcf --outline /path/to/outline.ttf

sources.json pins upstream archives and patches. The HarfBuzz fork retains its
published history and merges upstream 14.1.0 on the domed-bitmap branch. Release
builds use -O3 and local CPU tuning unless --portable is supplied. Every installed
runtime includes source archives, patches, licenses and checksums. The checker
verifies loaded library paths, policy probes and manifest hashes.

Fedora/Nobara system deployment uses package-system-bitmap-rpms.py. Supply tested
64-bit and 32-bit prefixes and original installed-version RPMs. It retains each
installed package's payload, ABI and version-aligned dependencies, replaces only
matching runtime libraries/headers, and checks the complete RPM transaction.
The authenticated installer retains rollback RPMs and the prior private runtime.
No desktop, display manager, application or XR session is restarted.

Dome Browser defaults to these shared libraries, obtains the exact selected host
bitmap face and native pixel size, and does not download or ship page fonts.
Non-bitmap page support is an explicit `outline-fonts` Cargo/build.py option,
which opts into bundled upstream FreeType/HarfBuzz and the font sanitizer.
FreeType/Cairo standalone source builds can opt out of the bitmap restriction
using FT_CONFIG_OPTION_ALLOW_NON_BITMAP_FONTS / CAIRO_ALLOW_NON_BITMAP_FONTS.
These options are never enabled in the DomeD runtime.
