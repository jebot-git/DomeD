#!/usr/bin/env python3
"""Exercise the installed standalone session on the private VKMS test device."""
import os
from pathlib import Path
import sys
script=Path(__file__).with_name("test-native-drm.py")
os.execv(sys.executable,[sys.executable,str(script),"--installed"])
