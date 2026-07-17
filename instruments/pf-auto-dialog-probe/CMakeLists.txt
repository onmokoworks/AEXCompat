add_library(pf_auto_dialog_probe MODULE pf_auto_dialog_probe.cpp)
target_include_directories(pf_auto_dialog_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
set_target_properties(pf_auto_dialog_probe PROPERTIES PREFIX "" SUFFIX ".aex")
