from _native_selftest import run


def test_launch_timeline_reaches_global_and_params_setup():
    run(
        "worker_effect_bootstrap_timing_selftest.exe",
        "worker_effect_bootstrap_timing_selftest",
    )
