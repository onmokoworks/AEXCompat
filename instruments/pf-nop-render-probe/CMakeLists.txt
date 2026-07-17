add_library(pf_nop_render_probe MODULE pf_nop_render_probe.cpp)
target_include_directories(pf_nop_render_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
set_target_properties(pf_nop_render_probe PROPERTIES PREFIX "" SUFFIX ".aex")
