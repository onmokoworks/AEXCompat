foreach(target pf_visual_audio_advertised_probe pf_visual_audio_unadvertised_probe
    pf_visual_audio_audio_only_probe pf_visual_audio_cluster_video_probe
    pf_visual_audio_sidecar_probe pf_visual_audio_layer_sidecar_probe
    pf_visual_audio_boundary_probe
    pf_visual_audio_format_probe pf_visual_audio_multi_handle_probe
    pf_visual_audio_double_checkin_probe)
  # The .rc carries the PiPL the worker needs to resolve EffectMain (issue #84);
  # without it every target here is rejected as unknown_no_effect_entrypoint.
  add_library(${target} MODULE pf_visual_audio_probe.cpp pf_visual_audio_probe.rc)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()
target_compile_definitions(pf_visual_audio_audio_only_probe PRIVATE
  AEXCOMPAT_AUDIO_EFFECT_ONLY=1)
target_compile_definitions(pf_visual_audio_cluster_video_probe PRIVATE
  AEXCOMPAT_CLUSTER_VIDEO_PROBE=1)
# The CI probe script builds the sidecar target as this fixture family's
# representative artifact. Keep the cluster fixtures in that same built set so
# the production cluster behavioral test cannot silently skip in CI.
add_dependencies(pf_visual_audio_sidecar_probe
  pf_visual_audio_audio_only_probe pf_visual_audio_cluster_video_probe)
target_compile_definitions(pf_visual_audio_advertised_probe PRIVATE AEXCOMPAT_ADVERTISE_AUDIO=1)
target_compile_definitions(pf_visual_audio_sidecar_probe PRIVATE
  AEXCOMPAT_ADVERTISE_AUDIO=1 AEXCOMPAT_REQUIRE_AUDIO_SIDECAR=1)
target_compile_definitions(pf_visual_audio_layer_sidecar_probe PRIVATE
  AEXCOMPAT_ADVERTISE_AUDIO=1 AEXCOMPAT_REQUIRE_AUDIO_SIDECAR=1
  AEXCOMPAT_AUDIO_LAYER_PROBE=1)
target_compile_definitions(pf_visual_audio_boundary_probe PRIVATE
  AEXCOMPAT_ADVERTISE_AUDIO=1 AEXCOMPAT_REQUIRE_AUDIO_SIDECAR=1
  AEXCOMPAT_AUDIO_BOUNDARY_PROBE=1)
target_compile_definitions(pf_visual_audio_format_probe PRIVATE
  AEXCOMPAT_ADVERTISE_AUDIO=1 AEXCOMPAT_REQUIRE_AUDIO_SIDECAR=1
  AEXCOMPAT_AUDIO_FORMAT_PROBE=1)
target_compile_definitions(pf_visual_audio_multi_handle_probe PRIVATE
  AEXCOMPAT_ADVERTISE_AUDIO=1 AEXCOMPAT_REQUIRE_AUDIO_SIDECAR=1
  AEXCOMPAT_AUDIO_MULTI_HANDLE_PROBE=1)
target_compile_definitions(pf_visual_audio_double_checkin_probe PRIVATE
  AEXCOMPAT_ADVERTISE_AUDIO=1 AEXCOMPAT_REQUIRE_AUDIO_SIDECAR=1
  AEXCOMPAT_AUDIO_DOUBLE_CHECKIN_PROBE=1)
