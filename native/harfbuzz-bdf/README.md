Current BDF/PCF runtime: see [../bitmap-runtime/README.md](../bitmap-runtime/README.md).
The pinned 12.3.2 BDF-only files below are retained as historical provenance.

# DomeD native BDF HarfBuzz fork

This is a [separate HarfBuzz fork](https://github.com/jebot-git/harfbuzz/tree/domed-bdf)
based on HarfBuzz 12.3.2. `fork.json` pins the published BDF branch commit,
upstream base and review-patch checksum. It admits raw Unicode BDF strikes
through both the blob and FreeType constructors. FreeType supplies the cmap,
advances, font metrics and ink extents; HarfBuzz supplies text shaping and
clusters. No SFNT wrapper or bitmap resampling is used.

Outline fonts, collections, PCF and table-callback font construction are rejected.
Empty-face results are intentional, including for valid TTF/OTF fonts. An
arbitrary caller can still supply its own font callbacks; this is a supported
font-format policy, not a security sandbox.

The companion Pango patch rejects scalable/non-BDF Fontconfig patterns and
uses the fork's native font functions. It **does not** use the older Pango BDF
callback workaround. DomeD's UI and session Fontconfig export also require
actual non-scalable BDF faces, regardless of filename.

Build a private runtime:

```sh
python3 scripts/prepare-bitmap-runtime.py --prefix "$PWD/build/bitmap-runtime"
python3 scripts/test-bitmap-runtime.py --prefix "$PWD/build/bitmap-runtime"
```

DomeD supports only this fork in its session. The session launcher verifies
both fork policy probes and the installed library checksums before starting the
compositor. Stock HarfBuzz or a mismatched Pango runtime fails the session check.
The default manifest is `/usr/share/domed-bdf-runtime/manifest.json`; an explicit
`DOME_BDF_RUNTIME_PREFIX` selects another complete build of the same fork.

System deployment uses locally versioned HarfBuzz/Pango Debian packages, with
original packages saved for rollback. The APT policy in
`packaging/90-domed-bdf-runtime.pref` selects only the DomeD fork versions.
Non-bitmap application rendering is outside the supported session policy.
The Graphite2 outline backend is not built; its two font accessors are omitted.

The source checkout, exact patches, licenses and build manifest accompany the
private runtime. Updating HarfBuzz requires porting the patch and rerunning the
positive bitmap and negative outline tests. See `test-bdf.cc` for the direct
HarfBuzz tests; the runtime tests additionally compare Pango/Cairo pixels with
FreeType and check loader paths.

The fork retains HarfBuzz's shaping/OT API implementation for ABI compatibility;
its font-loading entry points do not admit outline fonts. This is not removal of
HarfBuzz itself and does not claim complete complex-script bitmap coverage.

## System deployment

After the private runtime passes its tests, build matched packages and install
only the reviewed plan:

```sh
/usr/bin/python3 scripts/package-system-bitmap-runtime.py --prefix "$PWD/build/bitmap-runtime"
sudo -A /usr/bin/python3 scripts/install-system-bitmap-runtime.py
/usr/local/libexec/domed/check-bitmap-runtime
python3 scripts/test-bitmap-runtime.py --prefix /usr
```

The packager needs `python3-apt` and downloads the exact currently installed
versions for rollback. For local installed versions not available from APT,
pass `--original-archives DIR`. The plan upgrades the fourteen HarfBuzz/Pango
runtime, SDK, introspection and utility packages together. Only the core,
subset, Pango, PangoFT2 and PangoCairo shared objects change; companion binary
versions remain aligned. Installation preserves APT manual/automatic marks,
backs up the session launcher and packages, and installs the required policy
check and fork-only APT pin. It does not force a running session to restart.
New logins load the replacement; existing processes retain old mappings until
restarted. Rebuilding against new upstream releases requires updating the fork
lock, package versions and rerunning the bitmap tests.
