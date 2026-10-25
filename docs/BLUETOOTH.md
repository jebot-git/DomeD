# Bluetooth manager

Open **Bluetooth…** from DomeD's tray applications menu, choose the Bluetooth
page in the shared manager, use **View → Bluetooth** / **Alt+5**, or run
`dome-tray --bluetooth`. Additional invocations use the existing manager instance.
Bluetooth also has a permanent entry in the Tray applications list. Its placement
is fixed inside the manager; it does not create another tray icon.

Select a local adapter, then use **Power on** and **Scan (60 sec)** to find
nearby devices. Opening the page never powers on a radio or starts discovery.
**Visible (3 min)** lets other devices find this computer for a bounded time;
**Allow pairing** controls the adapter's pairable property. Adapter and device
changes arrive through BlueZ signals, including additions/removals and service
restarts. The selected adapter filters the device list. Battery percentage and
signal strength appear when the device exposes them.

Select a device to pair, connect/disconnect, trust/untrust, block/unblock, or
remove its saved pairing. Removal asks for confirmation. Pairing does not
implicitly trust a device. PIN and passkey requests use native masked inputs;
confirmation shows the device name, address and six-digit code. Display-only
codes explain what to type on the remote device. Incoming pairing and service
requests open the same page and require explicit approval. Escape or closing
the window cancels pending pairing and stops the manager's discoveries while
keeping its tray entry alive. Leaving the page also releases its discovery
sessions. Scans from other Bluetooth clients remain their own sessions.

The backend talks to `org.bluez` on the **system bus** with a private GIO
connection and a GLib event loop on its own thread. Pairing is asynchronous and
can be canceled while outstanding. The `KeyboardDisplay` agent accepts requests
only from the current BlueZ unique owner; stale responses are ignored and
unanswered requests expire after two minutes. It registers as the default agent
for incoming requests, reporting permission errors while retaining outgoing
pairing if registration permits it. The connection's lifetime owns its agent
and discovery sessions. No `bluetoothctl`, GTK or Qt frontend is launched.

The host needs a running BlueZ `bluetoothd` and a Bluetooth adapter. Device
permissions follow the host's system-bus/BlueZ policy. Missing service, missing
adapter and operation errors appear in the page. A blocked power-on action can
require changing airplane mode or a hardware radio switch; the manager does not
change the host's rfkill policy or install/start system services automatically.

Build the optimized FLTK release with `scripts/build-tray.sh` and stage using
`scripts/stage-tray.sh`. The stage/install scripts include the hidden
`org.domed.Bluetooth.desktop` metadata and this documentation. The page uses the
same GPU window, live appearance, universal menu and Wayland/X11 fallback as the
other suite pages, and works inside the desktop and XR manager window.

Tests use fake BlueZ adapters and a private bus, without touching host radios:

```sh
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next \
  python tests/tray/test_bluetooth.py
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next \
  --with python-xlib --with pillow python tests/tray/test_ui.py
```

The backend regression covers adapter selection/properties, discovery ownership,
visibility timeout, battery/signal values, all pairing interaction types, bad and
stale replies, sender authentication, cancel during pairing, connection/trust/
block/removal, hotplug and BlueZ restart. The rendered UI test exercises the
Bluetooth tray menu, power, selection, pairing confirmation, discovery and
close/restore alongside the existing pages. Real hardware compatibility and
headset presentation still require testing on those devices.

Protocol references: [BlueZ adapter](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Adapter.rst),
[device](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Device.rst), and
[agent](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Agent.rst) APIs.
