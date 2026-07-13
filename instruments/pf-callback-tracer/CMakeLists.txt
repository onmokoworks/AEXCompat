add_library(pf_callback_tracer MODULE pf_callback_tracer.cpp)
target_include_directories(pf_callback_tracer PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
target_link_libraries(pf_callback_tracer PRIVATE aexcompat_trace_writer)
set_target_properties(pf_callback_tracer PROPERTIES PREFIX "" SUFFIX ".aex")
