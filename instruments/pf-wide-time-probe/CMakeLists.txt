foreach(target pf_wide_time_allowed_probe pf_wide_time_denied_probe
    pf_automatic_wide_time_allowed_probe pf_automatic_wide_time_denied_probe)
  add_library(${target} MODULE pf_wide_time_probe.cpp)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()
target_compile_definitions(pf_wide_time_allowed_probe PRIVATE AEXCOMPAT_ADVERTISE_WIDE_TIME=1)
target_compile_definitions(pf_automatic_wide_time_allowed_probe PRIVATE
  AEXCOMPAT_SMART_WIDE_TEST=1 AEXCOMPAT_ADVERTISE_AUTOMATIC_WIDE_TIME=1)
target_compile_definitions(pf_automatic_wide_time_denied_probe PRIVATE
  AEXCOMPAT_SMART_WIDE_TEST=1 AEXCOMPAT_EXPECT_SMART_DENIED=1)
