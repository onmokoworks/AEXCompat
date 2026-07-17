add_library(pf_path_param_probe MODULE pf_path_param_probe.cpp)
add_library(pf_path_param_default_none_probe MODULE pf_path_param_probe.cpp)
add_library(pf_path_param_default_second_probe MODULE pf_path_param_probe.cpp)
add_library(pf_path_param_default_missing_probe MODULE pf_path_param_probe.cpp)
target_compile_definitions(pf_path_param_default_none_probe PRIVATE
  AEXCOMPAT_PATH_DEFAULT=0 AEXCOMPAT_EXPECT_PATH_ID=0)
target_compile_definitions(pf_path_param_default_second_probe PRIVATE
  AEXCOMPAT_PATH_DEFAULT=2 AEXCOMPAT_EXPECT_PATH_ID=2)
target_compile_definitions(pf_path_param_default_missing_probe PRIVATE
  AEXCOMPAT_PATH_DEFAULT=3 AEXCOMPAT_EXPECT_PATH_ID=0)
target_compile_definitions(pf_path_param_probe PRIVATE
  AEXCOMPAT_PATH_DEFAULT=0 AEXCOMPAT_EXPECT_PATH_ID=2)
foreach(target pf_path_param_probe pf_path_param_default_none_probe
    pf_path_param_default_second_probe pf_path_param_default_missing_probe)
target_include_directories(pf_path_param_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util")
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util")
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()
