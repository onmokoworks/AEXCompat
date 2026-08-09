"""Parameter checkouts must be answerable at the frame the host is rendering.

`checkout_param` refuses a checkout whose time is not the frame's unless the
plug-in advertised wide time input. The smart path serves checkouts from the
hosted ledger rather than from a classic dispatch context, and nothing set that
ledger's frame time: it kept `current_time = 0` / `current_time_scale = 1`, so
the gate admitted t=0 and refused every other time.

Every SmartFX frame past t=0 therefore had its first parameter checkout answered
with 4, which the plug-in returned as PF_Err_OUT_OF_MEMORY. AviUtl2 renders at
the timeline cursor, so no smart effect rendered anywhere but frame 0 - the
symptom that survived the two fixes in issue #777 (issue #828).

The native self-test drives the gate directly, so it needs no plug-in and no
AEX - only the build.
"""

from _native_selftest import run


def test_a_checkout_is_answerable_at_the_frame_being_rendered():
    run("worker_param_checkout_time_selftest.exe", "param_checkout_time_selftest")
