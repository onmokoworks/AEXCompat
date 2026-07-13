add_library(pf_crashkit MODULE pf_crashkit.cpp)
target_include_directories(pf_crashkit PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
target_link_libraries(pf_crashkit PRIVATE aexcompat_trace_writer)
set_target_properties(pf_crashkit PROPERTIES PREFIX "" SUFFIX ".aex")
