"""The classic lifecycle must not dispatch RENDER into a refused frame.

`classic_execution::begin_lifecycle` used to hand the caller a clean
`LifecycleResult` even when the plug-in's own SEQUENCE_SETUP or FRAME_SETUP had
refused, because `begin` returns the lifecycle opaquely and nothing read the
refusal back out. `dispatch_render`'s short-circuit keys on that error, so it
did not fire and RENDER ran against frame-local state `begin_frame` returns
before transferring.

Eight AE 2026 effects (PSL_Drop_Shadow, Basic_3D, PSL_Inner_Glow,
PSL_Inner_Shadow, PSL_Outer_Glow, Bulge, Spherize, Corner_Pin) took an access
violation in RENDER that way, which masked the FRAME_SETUP fault underneath:
`last_seh_selector` reported RENDER because the second crash overwrote the
first. Issue #725.

The native self-test drives the same functions with counting fakes, so it needs
no plug-in and no AEX - only the build.
"""

from _native_selftest import run


def test_a_refused_setup_stops_the_classic_dispatch():
    run("worker_classic_execution_selftest.exe", "classic_execution_selftest")
