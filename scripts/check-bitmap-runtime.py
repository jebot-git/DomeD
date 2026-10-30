#!/usr/bin/env python3
"""Refuse a DomeD session using a stock or mismatched font runtime."""
import argparse, ctypes as C, hashlib, json, pathlib

def check(manifest):
    policy = json.loads(manifest.read_text())
    if policy.get('policy') != 'native-unicode-bitmap-only':
        raise RuntimeError('DomeD requires the native BDF/PCF font runtime')
    if policy.get('formats') != ['BDF', 'PCF']:
        raise RuntimeError('DomeD requires both BDF and PCF')
    class DlInfo(C.Structure):
        _fields_ = [('name', C.c_char_p), ('base', C.c_void_p), ('symbol', C.c_char_p), ('address', C.c_void_p)]
    dladdr = C.CDLL(None).dladdr
    dladdr.argtypes = [C.c_void_p, C.POINTER(DlInfo)]; dladdr.restype = C.c_int
    loaded = {}
    symbols = {'libfreetype.so.6': 'FT_Dome_Bitmap_Only', 'libcairo.so.2': 'cairo_dome_bitmap_only', 'libharfbuzz.so.0': 'hb_dome_bitmap_only', 'libpango-1.0.so.0': 'pango_version',
               'libpangoft2-1.0.so.0': 'pango_dome_bitmap_only', 'libpangocairo-1.0.so.0': 'pango_cairo_font_map_get_default'}
    for name, symbol in symbols.items():
        lib = C.CDLL(name); fn = getattr(lib, symbol)
        if symbol.endswith(('bitmap_only', 'Bitmap_Only')):
            fn.argtypes = []; fn.restype = C.c_int
            if fn() != 1: raise RuntimeError(name + ' is not the bitmap fork')
        formats = {'libfreetype.so.6': 'FT_Dome_Bitmap_Formats', 'libharfbuzz.so.0': 'hb_dome_bitmap_formats'}.get(name)
        if formats:
            probe = getattr(lib, formats); probe.argtypes = []; probe.restype = C.c_uint
            if probe() != 3: raise RuntimeError(name + ' must support native BDF and PCF')
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
