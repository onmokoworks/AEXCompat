foreach(target pf_input_write_allowed_probe pf_input_write_denied_probe)
  add_library(${target} MODULE pf_input_write_probe.cpp ${target}.rc)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()
target_compile_definitions(pf_input_write_allowed_probe PRIVATE AEXCOMPAT_ADVERTISE_INPUT_WRITE=1)
