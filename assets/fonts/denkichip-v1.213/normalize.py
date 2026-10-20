"""Normalize upstream BDF framing and metadata; retain every glyph verbatim."""
from pathlib import Path
import sys

source = Path(sys.argv[1]).read_text()
assert source.startswith("STARTFONT 2.1\nFONT x10y12pxDenkiChipHangul\nSIZE 12 75 75\n")
assert "STARTPROPERTIES 2\n" in source
normalized = source.replace(
    "FONT x10y12pxDenkiChipHangul",
    "FONT -quiple-x10y12pxDenkiChipHangul-Regular-R-Normal--12-120-72-72-p-100-iso10646-1",
    1,
).replace("SIZE 12 75 75", "SIZE 12 72 72", 1).replace(
    "STARTPROPERTIES 2",
    'STARTPROPERTIES 8\nFAMILY_NAME "x10y12pxDenkiChipHangul"\n'
    'WEIGHT_NAME "Regular"\nCHARSET_REGISTRY "ISO10646"\n'
    'CHARSET_ENCODING "1"\nPIXEL_SIZE 12\nPOINT_SIZE 120',
    1,
).rstrip() + "\n"
assert normalized.split("\nCHARS ", 1)[1].rstrip() == source.split("\nCHARS ", 1)[1].rstrip()
Path(sys.argv[2]).write_text(normalized)
