#!/usr/bin/env python3
"""Refuse a DomeD session using a stock or mismatched font runtime."""
import argparse, ctypes as C, hashlib, json, pathlib

def check(manifest):
    policy = json.loads(manifest.read_text())
    if policy.get('policy') != 'native-unicode-bdf-only':
        raise RuntimeError('DomeD requires the native BDF-only font runtime')
    class DlInfo(C.Structure):
        _fields_ = [('name', C.c_char_p), ('base', C.c_void_p), ('symbol', C.c_char_p), ('address', C.c_void_p)]
    dladdr = C.CDLL(None).dladdr
    dladdr.argtypes = [C.c_void_p, C.POINTER(DlInfo)]; dladdr.restype = C.c_int
    loaded = {}
    symbols = {'libharfbuzz.so.0': 'hb_dome_bdf_only', 'libpango-1.0.so.0': 'pango_version',
               'libpangoft2-1.0.so.0': 'pango_dome_bdf_only', 'libpangocairo-1.0.so.0': 'pango_cairo_font_map_get_default'}
    for name, symbol in symbols.items():
        lib = C.CDLL(name); fn = getattr(lib, symbol)
        if symbol.endswith('bdf_only'):
            fn.argtypes = []; fn.restype = C.c_int
            if fn() != 1: raise RuntimeError(name + ' is not the BDF fork')
        info = DlInfo()
        if not dladdr(C.cast(fn, C.c_void_p), C.byref(info)): raise RuntimeError('Cannot locate ' + name)
        path = pathlib.Path(info.name.decode()).resolve()
        if hashlib.sha256(path.read_bytes()).hexdigest() != policy['libraries'][name]:
            raise RuntimeError('DomeD font runtime differs from its installed manifest: ' + str(path))
        loaded[name] = str(path)
    return loaded

if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest', type=pathlib.Path, default=pathlib.Path('/usr/share/domed-bdf-runtime/manifest.json'))
    args = p.parse_args()
    try:
        print(json.dumps(check(args.manifest), indent=2))
    except (OSError, AttributeError, KeyError, ValueError, RuntimeError) as e:
        p.exit(1, 'DomeD bitmap-only runtime check failed: ' + str(e) + '\n')
