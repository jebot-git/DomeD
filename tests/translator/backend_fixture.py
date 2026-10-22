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
else:
    print(json.dumps({'ok': False, 'error': 'Unexpected test operation'}))
