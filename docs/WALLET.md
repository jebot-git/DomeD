# Qt-free wallet backend

DomeD's FLTK wallet manager uses KWallet's public D-Bus API through GIO; it does
not need Qt. The [`kwallet-xr`](https://github.com/jebot-git/kwallet-xr) source fork supplies a native `dome-walletd`
backend using GLib/GIO, libgcrypt and GPGME, retaining the public KWallet,
Secret Service and Secret portal interfaces and existing encrypted wallet files.
Its standard build does not discover or link Qt/KDE libraries. The old Qt build
is an explicit `DOME_QT_FREE=OFF` compatibility option in that fork.

Session startup preserves any provider that already owns
`org.freedesktop.secrets`. Otherwise `DOME_WALLET_BACKEND` selects:

| Value | Startup policy |
| --- | --- |
| `auto` (default) | Prefer `/usr/local/libexec/domed/dome-walletd`, then packaged `/usr/libexec/domed/dome-walletd`; fall back to the existing `/usr/bin/ksecretd` when native is not installed. |
| `native` | Start only the Qt-free backend; never fall back to Qt. |
| `kwallet` | Explicitly use the existing KWallet daemon. |
| `none` | Do not start a provider. |

Unknown values also leave provider startup disabled. D-Bus activation is a
separate installation concern: the native fork stages matching activation files
for KWallet, Secret Service and the existing `kwallet` Secret portal name.

No automatic wallet migration, password export or live provider takeover is
performed. PAM still needs to launch the selected backend: the fork implements
the existing `--pam-login HASH_FD ENV_FD` protocol, but installing a new binary
alone does not change a distributor's PAM module's executable path. Keep the
matching PAM/activation configuration and a rollback package when deploying.

Existing Qt applications keep their own Qt/KWallet client-library dependencies;
the Qt-free target is the backend process and DomeD integration, not those
applications' C++ ABI. Raw wallets, passwords and generated test material must
remain outside Git. Validation uses private buses/HOME directories and disposable
wallets, including independent original-KWallet file-format comparisons.

The native replacement RPM keeps `/usr/bin/ksecretd` as a wrapper. Existing
DomeD installations and PAM's compiled-in launch path therefore also reach the
native backend without changing PAM authentication policy. Installation leaves
running providers in place until the next normal session start.
