"""FRAME_SETUP must be handed the output extent it is allowed to revise.

`PF_OutData::width/height` is where an effect that declared
PF_OutFlag_I_EXPAND_BUFFER states the enlarged output it wants. It is not only a
place to write: AE's own Basic_3D derives its answer from what it finds there.
The host used to leave those fields zero at FRAME_SETUP, so Basic_3D answered
1x1 - a shrink it had never declared PF_OutFlag_I_SHRINK_BUFFER for - the host's
output-bounds check refused the resize, and the frame died as an
output-validation failure with RENDER never dispatched. Given the extent it is
being offered, Basic_3D answers that extent and the frame renders. Issue #984.

The native self-test drives `begin_frame` with a recording fake, so it needs no
plug-in and no AEX - only the build.
"""

from _native_selftest import run


def test_frame_setup_is_offered_the_output_extent_it_may_revise():
    run("worker_render_lifecycle_selftest.exe", "render_lifecycle_selftest")
