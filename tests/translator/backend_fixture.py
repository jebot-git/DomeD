#!/usr/bin/env python3
"""Deterministic worker for the UI tests; no external HTTP or user data."""
import json
import sys
import time
x = json.load(sys.stdin)
if x['action'] == 'translate':
    time.sleep(.25)
    print(json.dumps({'ok': True, 'text': '日本語', 'reading': 'nihongo',
                      'source_reading': '', 'detected': 'en', 'notice': ''}))
elif x['action'] == 'paste':
    import importlib.util
    from pathlib import Path
    source = Path(__file__).resolve().parents[2] / 'native/translator/backend.py'
    spec = importlib.util.spec_from_file_location('selection_transport', source)
    backend = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(backend)
    try:
        print(json.dumps({'ok': True, **backend.run(x)}))
    except Exception:
        print(json.dumps({'ok': False, 'error': 'Empty selection'}))
else:
    print(json.dumps({'ok': False, 'error': 'Unexpected test operation'}))
