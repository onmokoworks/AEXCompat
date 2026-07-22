if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify a reviewed After Effects SDK")
endif()

add_library(issue60_delay_transitive SHARED delay_transitive.cpp)
set_target_properties(issue60_delay_transitive PROPERTIES PREFIX "")

add_library(issue60_delay_dependency SHARED delay_dependency.cpp)
target_link_libraries(issue60_delay_dependency PRIVATE issue60_delay_transitive)
set_target_properties(issue60_delay_dependency PROPERTIES PREFIX "")

add_library(pf_delay_load_probe MODULE pf_delay_load_probe.cpp)
target_include_directories(pf_delay_load_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
target_link_libraries(pf_delay_load_probe PRIVATE
  issue60_delay_dependency
  delayimp
)
if(MSVC)
  target_link_options(pf_delay_load_probe PRIVATE
    "/DELAYLOAD:issue60_delay_dependency.dll"
  )
endif()
set_target_properties(pf_delay_load_probe PROPERTIES PREFIX "" SUFFIX ".aex")

add_executable(issue60_delay_load_gate delay_load_gate.cpp)
