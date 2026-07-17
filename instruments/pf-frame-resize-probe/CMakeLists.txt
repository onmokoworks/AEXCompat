foreach(target
    pf_expand_allowed_probe pf_expand_denied_probe
    pf_shrink_allowed_probe pf_shrink_denied_probe)
  add_library(${target} MODULE pf_frame_resize_probe.cpp)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()
target_compile_definitions(pf_expand_allowed_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=4 AEXCOMPAT_RESIZE_FLAG=PF_OutFlag_I_EXPAND_BUFFER)
target_compile_definitions(pf_expand_denied_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=4 AEXCOMPAT_EXPECT_DENIED=1)
target_compile_definitions(pf_shrink_allowed_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=-4 AEXCOMPAT_RESIZE_FLAG=PF_OutFlag_I_SHRINK_BUFFER)
target_compile_definitions(pf_shrink_denied_probe PRIVATE
  AEXCOMPAT_RESIZE_DELTA=-4 AEXCOMPAT_EXPECT_DENIED=1)
