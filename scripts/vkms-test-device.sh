#!/usr/bin/env bash
# Disposable virtual DRM output, hidden from the running desktop and login manager.
set -euo pipefail
[[ $(id -u) -eq 0 ]] || { echo "Run using pkexec /usr/bin/bash $0 prepare|cleanup" >&2; exit 1; }
rule=/run/udev/rules.d/99-dome-vkms-test.rules
marker=/run/dome-vkms-test.loaded
case "${1:-}" in
    prepare)
        if [[ -d /sys/module/vkms && ! -f "$marker" ]]; then
            echo 'An existing VKMS installation is in use; refusing to change it.' >&2
            exit 1
        fi
        systemctl stop dome-vkms-test-cleanup.service 2>/dev/null || true
        install -d -m 0755 /run/udev/rules.d
        cat > "$rule" <<'RULE'
SUBSYSTEM=="drm", KERNELS=="vkms*", ENV{ID_SEAT}="dome-test", TAG-="seat", TAG-="master-of-seat"
SUBSYSTEM=="drm", KERNELS=="dome-test*", ENV{ID_SEAT}="dome-test", TAG-="seat", TAG-="master-of-seat"
RULE
        udevadm control --reload-rules
        modprobe vkms create_default_dev=0
        touch "$marker"
        device=/sys/kernel/config/vkms/dome-test
        if [[ ! -d "$device" ]]; then
            mkdir "$device"
            for suffix in 1 2; do
                mkdir "$device/planes/primary$suffix" "$device/crtcs/crtc$suffix" "$device/encoders/encoder$suffix" "$device/connectors/display$suffix"
                echo 1 > "$device/planes/primary$suffix/type"
                echo 1 > "$device/connectors/display$suffix/status"
                ln -s "$device/crtcs/crtc$suffix" "$device/planes/primary$suffix/possible_crtcs/crtc$suffix"
                ln -s "$device/crtcs/crtc$suffix" "$device/encoders/encoder$suffix/possible_crtcs/crtc$suffix"
                ln -s "$device/encoders/encoder$suffix" "$device/connectors/display$suffix/possible_encoders/encoder$suffix"
            done
            echo 1 > "$device/enabled"
        else
            echo 0 > "$device/enabled"
            udevadm settle
            echo 1 > "$device/enabled"
        fi
        for card in /sys/class/drm/card[0-9]*; do
            [[ "$card" != *-* ]] || continue
            if [[ "$(basename -- "$(readlink -f "$card/device")")" == vkms* ]]; then
                udevadm trigger --action=change "$card"
            fi
        done
        udevadm settle
        echo 'Private VKMS test output ready; physical displays and input devices are untouched.'
        ;;
    disconnect-second|connect-second)
        device=/sys/kernel/config/vkms/dome-test
        [[ -d "$device/connectors/display2" ]] || exit 1
        if [[ "$1" == disconnect-second ]]; then echo 2 > "$device/connectors/display2/status"; else echo 1 > "$device/connectors/display2/status"; fi
        ;;
    prepare-second-gpu)
        [[ -f "$marker" ]] || exit 1
        sed -i 's/KERNELS=="dome-test",/KERNELS=="dome-test*",/' "$rule"
        udevadm control --reload-rules
        device=/sys/kernel/config/vkms/dome-test-gpu2
        if [[ -d "$device" ]]; then
            echo 0 > "$device/enabled"
            rm -- "$device/planes/primary/possible_crtcs/crtc" "$device/encoders/encoder/possible_crtcs/crtc" "$device/connectors/display/possible_encoders/encoder"
            rmdir "$device/planes/primary" "$device/crtcs/crtc" "$device/encoders/encoder" "$device/connectors/display" "$device"
        fi
        if [[ ! -d "$device" ]]; then
            mkdir "$device"
            mkdir "$device/planes/primary" "$device/crtcs/crtc" "$device/encoders/encoder" "$device/connectors/display"
            echo 1 > "$device/planes/primary/type"
            echo 1 > "$device/connectors/display/status"
            ln -s "$device/crtcs/crtc" "$device/planes/primary/possible_crtcs/crtc"
            ln -s "$device/crtcs/crtc" "$device/encoders/encoder/possible_crtcs/crtc"
            ln -s "$device/encoders/encoder" "$device/connectors/display/possible_encoders/encoder"
            echo 1 > "$device/enabled"
        fi
        udevadm settle
        ;;
    cleanup)
        if [[ -f "$marker" ]]; then
            for device in /sys/kernel/config/vkms/dome-test /sys/kernel/config/vkms/dome-test-gpu2; do
            if [[ -d "$device" ]]; then
                echo 0 > "$device/enabled"
                for link in "$device"/planes/*/possible_crtcs/* "$device"/encoders/*/possible_crtcs/* "$device"/connectors/*/possible_encoders/*; do
                    [[ ! -L "$link" ]] || rm -- "$link"
                done
                for object in "$device"/planes/* "$device"/encoders/* "$device"/connectors/* "$device"/crtcs/*; do rmdir "$object"; done
                rmdir "$device"
            fi
            done
            # Older/default VKMS instances can have been discovered by Xwayland.
            # Notify userspace to release only those virtual device descriptors.
            for card in /sys/class/drm/card[0-9]*; do
                [[ "$card" != *-* ]] || continue
                if [[ "$(basename -- "$(readlink -f "$card/device")")" == vkms ]]; then
                    udevadm trigger --action=remove "$card"
                fi
            done
            udevadm settle
            if modprobe -r vkms; then
                rm -f -- "$marker" "$rule"
                udevadm control --reload-rules
                echo 'Private VKMS test output removed.'
            else
                # Mesa/Xwayland may cache a descriptor to the initial virtual
                # card. Keep it hidden and unload after that session exits.
                for card in /sys/class/drm/card[0-9]*; do
                    [[ "$card" != *-* ]] || continue
                    if [[ "$(basename -- "$(readlink -f "$card/device")")" == vkms ]]; then
                        udevadm trigger --action=change "$card"
                    fi
                done
                udevadm settle
                cat > /run/dome-vkms-test-cleanup.sh <<'CLEANUP'
#!/usr/bin/bash
while [[ -d /sys/module/vkms ]]; do
    if /usr/sbin/modprobe -r vkms 2>/dev/null; then break; fi
    sleep 15
done
rm -f /run/dome-vkms-test.loaded /run/udev/rules.d/99-dome-vkms-test.rules /run/dome-vkms-test-cleanup.sh
/usr/bin/udevadm control --reload-rules
CLEANUP
                chmod 0700 /run/dome-vkms-test-cleanup.sh
                systemd-run --unit=dome-vkms-test-cleanup --description='Remove temporary DomeD VKMS test driver after cached descriptors close' --collect /usr/bin/bash /run/dome-vkms-test-cleanup.sh
                echo 'Private test output removed. Cached VKMS driver will unload automatically after Xwayland exits.'
            fi
        fi
        ;;
    *) echo 'Usage: vkms-test-device.sh prepare|cleanup' >&2; exit 2 ;;
esac
