# X07 image and protocol fixtures

The four `black.*` files are real, independently encoded 3x2 RGB images with
six opaque black pixels. They exercise JPEG, BMP, TIFF and WebP decoders through
the Mac GUI and all resident depths. Generated with Pillow 12.2.0:

```python
from PIL import Image
image = Image.new("RGB", (3, 2), (0, 0, 0))
for extension in ("jpg", "bmp", "tiff", "webp"):
    image.save("black." + extension, lossless=True)
```

The byte fixtures are committed, so Pillow is not required to run tests.
`resident_transport.py` is a process protocol double, not an AEX emulator.
It changes geometry only when it receives the actual negotiation CLI option
and emits boundary samples for GUI export tests. The tests also run session
admission, process cleanup and exact output checks. Python 3 is required for
this double; it uses only the standard library.
