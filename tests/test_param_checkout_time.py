"""Parameter checkouts return the value at the requested time.

WIDE_TIME_INPUT declares a temporal cache dependency; it is not an admission
gate for checkout_param. The native self-test covers static and animated hosted
values, another-time checkout without that flag, and balanced checkin without
requiring an AEX or SDK.
"""

from _native_selftest import run


def test_a_checkout_is_answerable_at_the_frame_being_rendered():
    run("worker_param_checkout_time_selftest.exe", "param_checkout_time_selftest")
