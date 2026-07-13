if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify a reviewed After Effects SDK")
endif()

add_library(pf_null_echo MODULE pf_null_echo.cpp)
target_include_directories(pf_null_echo PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
target_link_libraries(pf_null_echo PRIVATE aexcompat_trace_writer)
set_target_properties(pf_null_echo PROPERTIES PREFIX "" SUFFIX ".aex")
