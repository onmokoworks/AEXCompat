foreach(target pf_shutter_dependency_probe pf_shutter_unadvertised_probe)
  add_library(${target} MODULE pf_shutter_dependency_probe.cpp)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()
target_compile_definitions(pf_shutter_dependency_probe PRIVATE AEXCOMPAT_ADVERTISE_SHUTTER=1)
