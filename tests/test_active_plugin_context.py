"""Spawned render threads inherit only an explicitly activated plug-in context.

The focused helper test proves normal, exceptional, and null restoration.  The
classic-runtime test additionally executes the shipping ``threaded_default``
dispatch and observes the active string-table/module pair at the boundary of
both real render threads, so deleting product-path propagation fails coverage.
"""

from _native_selftest import run


def test_spawned_render_threads_activate_and_restore_plugin_context():
    run(
        "worker_active_plugin_context_selftest.exe",
        "active_plugin_context_selftest",
    )


def test_threaded_default_dispatch_activates_plugin_context():
    run("worker_classic_runtime_selftest.exe", "classic_runtime_selftest")
