This intermediate scope is superseded by the qualified [poll/clipboard runtime](X11-CLIPBOARD-POLL.md). Its original evidence and negatives remain below.

# X11 clipboard ownership implementation — intermediate

The27th source patch isolates TARGETS queries from real Paste replies, preserves
unrelated events, frees every property payload, and uses fresh server timestamps
for explicit ownership claims. Paste negotiation retains its original requestor
and request time. Each response wait is bounded. All public headers/layouts stay
unchanged. This source checkpoint is not an accepted SDK.

The v2 SDK passes five independent two-client protocol modes (including concurrent
Paste/metadata, malformed type, refusal, late reply and repeated text/image
handoffs) in Release and owned ASan/UBSan/leaks. The ordinary canonical picker
matrix passes8 commands, actual current Sculpt passes60 checks, and actual native
Wayland projection/filename Return passes22 checks. Original25/26 false-ACK,
owner-transfer and206-byte property leak negatives remain preserved. An initial
probe font-name lifetime bug was fixed in the fixture, with its negative kept.

A genuine DISPLAY_FD1101 control exposed unsafe select fd_set use in the first
wait helper; v3 guards the fallback and supports poll. The same test then reveals
an inherited select overflow in FLTK's Unix dispatcher because the builder's
upstream default FLTK_USE_POLL is OFF. These negatives are explicit, not accepted
or suppressed. A new final child must enable the official poll option, prove it
from actual build configuration, and repeat affected SDK/clipboard/UI checks
before any canonical main or Sculpt adoption publication.

All prefixes and evidence under artifacts/clipboard-proof remain immutable.
No user desktop, system installation, physical headset or GPU was used.
