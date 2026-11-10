# Canonical clipboard, picker and poll runtime

The qualified runtime combines the compact picker 25, actual filename callback
reason 26 and clipboard ownership/query 27 patches. Ordinary native commands now
use real input authority: Paste does not inherit a prior dialog's Enter key;
copy-back must be owned by the actual application before it acknowledges a
filename. Actual Return still navigates directories and accepts valid files.

X11 clipboard metadata uses a dedicated requestor/property/target/time. It leaves
real Paste replies and unrelated native events queued, validates atom type,
format/count and current owner, and frees every XGetWindowProperty result.
Late metadata is discarded without becoming input data. Response-event waits
have 250ms deadlines. Ownership claims use an actual server PropertyNotify time
and are verified against the real server owner. Paste negotiation keeps its
original requestor/time. This follows the [Xlib selection rules](https://x.org/releases/current/doc/libX11/libX11/libX11.pdf):
old timestamps can make ownership requests have no effect. No stale event-time
or fabricated CurrentTime ownership shortcut is used.

The canonical builder enables the existing upstream `FLTK_USE_POLL=ON` option.
This resolves the demonstrated DISPLAY_FD1101 fd_set overflow within the host's
unchanged 4096 soft limit. There is no private event-loop fork. A small unused
zero-initialized fdsets compatibility definition retains the previous exported
static symbol; actual dispatch uses poll. All 165 public include files and the
6328-byte FileChooser layout remain unchanged, with no old export removed.

The installed runtime-profile.json binds actual generated private-config.h,
builder, complete patch chain and installed toolkit library digests. Consumers
must verify imported libraries resolve inside that profile's prefix and match
its digests. A source patch marker alone cannot prove a poll-enabled build.
The retained configuration and profile are provenance; they add no public field
or serialized scene capability.

Fresh owned poll-enabled bases and coherent source-triggered continuations build
Release and ASan/UBSan successfully. All owned toolkit/vendored-libdecor C/C++ is
instrumented in the sanitizer profile; the canonical bitmap stack remains an
explicit external dependency. No foreign toolkit objects or suppressions are
used. Final immutable prefixes are in this worktree at
artifacts/toolkit-poll-final-release and toolkit-poll-final-sanitize.

Twelve final commands pass with 1,523 input hashes unchanged per command:

- Eight actual shared picker runs: X11/native Wayland × Release/SAN × normal
  1600×1000/font 16 and compact 640×360/font 32, retaining exact callback/file/
  Unicode/selection/preferences/Cancel and glyph/control-bound assertions.
- Five independent two-client clipboard modes per profile, 49 checks: concurrent
  real Paste/metadata and exact event delivery, malformed property types,
  refusal, delayed reply/timeout, and repeated text/image owner handoffs with
  exact public image-copy/paste pixels.
- A genuine display descriptor 1101 case per profile, 7 checks, with bounded late
  metadata and preserved input/events. Its select-based predecessors remain
  explicit sanitizer negatives, not accepted limitations.

The matching current Sculpt component additionally passes 15 actual universal
menu export/Open/Save/Cancel/Close checks per normal/compact R/S run (60 total),
plus 9 native Wayland projection/filename Return/paint/history/close checks
per profile (18 total). This reuses exact coherent c092 app/core/support with
identical public headers and actual new-runtime maps. It is an SDK-only delta,
not a new whole-Sculpt build or full production completion.

Earlier source/input/protocol failures remain preserved. They include the
original 960×600 chooser outside 640×360, stale callback acceptance, a sender-owned
false acknowledgment, strict owner-transfer failures, and 206 bytes in four
property allocations. The new protocol fixture's initial temporary font-name
lifetime error was corrected in the fixture; its sanitizer report remains
separate. The source-generation duplicate-patch staging error also remains
separate from runtime qualification.

The machine receipt is [validation/X11-CLIPBOARD-POLL.json](validation/X11-CLIPBOARD-POLL.json).
It pins exact logs, profiles, source/SDK/ABI manifests and before/after ledgers.
Tests use signed private Xvfb in an unprivileged namespace and a private nested
DomeD compositor/software GL. No user desktop, physical headset, system install
or session restart occurred. User stereo work on local main a5c444 remains exact.
