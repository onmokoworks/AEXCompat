from _native_selftest import run


def test_bulk_argb8_transport_preserves_pixels_guards_and_legacy_depths():
    run("worker_pixel_transport_selftest.exe", "worker_pixel_transport_selftest")
