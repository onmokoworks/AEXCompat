if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify a reviewed After Effects SDK")
endif()

add_library(pf_gpu_memory_probe MODULE pf_gpu_memory_probe.cpp)
target_include_directories(pf_gpu_memory_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
set_target_properties(pf_gpu_memory_probe PROPERTIES PREFIX "" SUFFIX ".aex")
