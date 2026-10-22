# Original font material

- `10x20.bdf` is the supplied ISO-8859-1 BDF carrying the NCD licence text.
- `iso/10x20.pcf.gz` is the broad X.Org Unicode PCF used for the TTF build.
- `iso/10x20-ISO8859-*.pcf.gz` and `10x20-KOI8-R.pcf.gz` preserve the
  encoding-specific X11 distributions.
- `fixed20-resource-fork.bin` is the recovered classic Mac resource fork.
  It contains a FOND resource named `Fixed` and the 6,768-byte `sfnt` converted
  by Christian Pagé in 2003.
- `fixed20-resource-fork-placeholder.gz` records the first attempted upload.
  Its data fork is empty because ordinary gzip did not preserve Mac resources.

`Fixed20-Original.ttf` is extracted without modification from the resource
fork. The separate Unicode TTF is generated from the full Unicode PCF.
