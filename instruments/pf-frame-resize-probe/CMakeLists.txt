cmake_minimum_required(VERSION 3.20)
project(pf_frame_resize_probe LANGUAGES CXX)

foreach(target
    pf_expand_allowed_probe pf_expand_denied_probe
    pf_shrink_allowed_probe pf_shrink_denied_probe)
  # Each variant compiles its own PiPL (${target}.rc) so display/match names
  # and OLGe out_flags are distinct per resize contract; a host that indexes
  # effects by match name then sees four identities, not one.
  add_library(${target} MODULE pf_frame_resize_probe.cpp ${target}.rc)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()
# The runtime out_flags come from these defs; each variant's PiPL OLGe is
# baked into its own ${target}.rc to match (expand 1536, shrink 5120, denied
# 1024).
target_compile_definitions(pf_expand_allowed_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=4 AEXCOMPAT_RESIZE_FLAG=PF_OutFlag_I_EXPAND_BUFFER)
target_compile_definitions(pf_expand_denied_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=4 AEXCOMPAT_EXPECT_DENIED=1)
target_compile_definitions(pf_shrink_allowed_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=-4 AEXCOMPAT_RESIZE_FLAG=PF_OutFlag_I_SHRINK_BUFFER)
target_compile_definitions(pf_shrink_denied_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=-4 AEXCOMPAT_EXPECT_DENIED=1)
