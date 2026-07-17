add_library(pf_arbitrary_scan_probe MODULE pf_arbitrary_scan_probe.cpp)
target_include_directories(pf_arbitrary_scan_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util")
set_target_properties(pf_arbitrary_scan_probe PROPERTIES PREFIX "" SUFFIX ".aex")
