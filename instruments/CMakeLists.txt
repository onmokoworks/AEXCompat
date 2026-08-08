cmake_minimum_required(VERSION 3.20)
project(AEXCompatInstruments LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(aexcompat_trace_writer STATIC common/trace_writer.cpp)
target_include_directories(aexcompat_trace_writer PUBLIC common)

add_executable(trace_writer_selftest common/trace_writer_selftest/main.cpp)
target_link_libraries(trace_writer_selftest PRIVATE aexcompat_trace_writer)

# abi_layout_probe is headers-only against the SDK and builds nothing that has
# to load into AE, so it is gated separately from the probe AEX set below. This
# file read AE_SDK_ROOT alone, so on a machine set up the way CLAUDE.md's
# canonical verification says - and on CI, which exports AFTER_EFFECTS_SDK_ROOT
# after checking the downloaded SDK against a pinned SHA256 - the probe was
# skipped and its static_asserts, the only thing grounding the generated ABI
# offsets in the real SDK structs, never compiled (issue #981).
#
# AFTER_EFFECTS_SDK_ROOT wins: a machine-scope AE_SDK_ROOT left over from local
# probe-AEX work must not silently outrank the pinned tree on a self-hosted
# runner. Both names are checked for the header the probe actually includes
# rather than for a directory that exists, so a variable pointing at the wrong
# tree gives a configure-time skip instead of an include-path error.
if(DEFINED ENV{AFTER_EFFECTS_SDK_ROOT} AND
   EXISTS "$ENV{AFTER_EFFECTS_SDK_ROOT}/Examples/Headers/AE_Effect.h")
  set(AEXCOMPAT_SDK_ROOT "$ENV{AFTER_EFFECTS_SDK_ROOT}")
elseif(DEFINED ENV{AE_SDK_ROOT} AND
       EXISTS "$ENV{AE_SDK_ROOT}/Examples/Headers/AE_Effect.h")
  set(AEXCOMPAT_SDK_ROOT "$ENV{AE_SDK_ROOT}")
endif()

# The probe AEX set below resolves AE_SDK_ROOT on its own, so two different
# trees would mean the ABI observation and the probe AEX were compiled against
# different SDKs in one configure - a difference nothing downstream records.
# Compared as real paths so a trailing slash or a mixed separator does not read
# as a second SDK.
if(DEFINED AEXCOMPAT_SDK_ROOT AND DEFINED ENV{AE_SDK_ROOT} AND
   EXISTS "$ENV{AE_SDK_ROOT}")
  get_filename_component(AEXCOMPAT_SDK_ROOT_REAL "${AEXCOMPAT_SDK_ROOT}" REALPATH)
  get_filename_component(AEXCOMPAT_AEX_SDK_ROOT_REAL "$ENV{AE_SDK_ROOT}" REALPATH)
  if(NOT "${AEXCOMPAT_SDK_ROOT_REAL}" STREQUAL "${AEXCOMPAT_AEX_SDK_ROOT_REAL}")
    message(WARNING
      "abi_layout_probe uses ${AEXCOMPAT_SDK_ROOT} while the probe AEX set uses "
      "$ENV{AE_SDK_ROOT}; point both at one SDK before trusting either output")
  endif()
endif()

if(DEFINED AEXCOMPAT_SDK_ROOT)
  add_executable(abi_layout_probe abi-layout-probe/main.cpp)
  target_include_directories(abi_layout_probe PRIVATE
    "${AEXCOMPAT_SDK_ROOT}/Examples/Headers"
    "${AEXCOMPAT_SDK_ROOT}/Examples/Headers/SP"
    "${AEXCOMPAT_SDK_ROOT}/Examples/Util"
  )
elseif(DEFINED ENV{AFTER_EFFECTS_SDK_ROOT} OR DEFINED ENV{AE_SDK_ROOT})
  # A variable that is set but does not name an SDK is a misconfiguration, not
  # an absent SDK. The gate used to be a directory check, so this case reached
  # the compiler and failed loudly on the missing include; keep it loud rather
  # than letting the observation-vs-probe check silently not run.
  message(WARNING
    "AFTER_EFFECTS_SDK_ROOT / AE_SDK_ROOT is set but does not hold "
    "Examples/Headers/AE_Effect.h; abi_layout_probe is skipped and "
    "tests/test_abi_layout_observation_matches_probe.py will skip with it")
else()
  message(STATUS
    "Neither AFTER_EFFECTS_SDK_ROOT nor AE_SDK_ROOT names an SDK with "
    "Examples/Headers/AE_Effect.h; abi_layout_probe is skipped")
endif()

# The probe AEX set keeps its own AE_SDK_ROOT gate: each subdirectory reads that
# variable itself and fails the configure without it, and CI configures them
# separately anyway, so widening this condition would move where they are built
# rather than whether they are.
if(DEFINED ENV{AE_SDK_ROOT} AND EXISTS "$ENV{AE_SDK_ROOT}")
  add_subdirectory(pf-null-echo)
  add_subdirectory(pf-delay-load-probe)
  add_subdirectory(pf-callback-tracer)
  add_subdirectory(pf-crashkit)
  add_subdirectory(pf-gpu-memory-probe)
  add_subdirectory(pf-path-data-probe)
  add_subdirectory(pf-auto-dialog-probe)
  add_subdirectory(pf-nop-render-probe)
  add_subdirectory(pf-input-write-probe)
  add_subdirectory(pf-frame-resize-probe)
  add_subdirectory(pf-wide-time-probe)
  add_subdirectory(pf-shutter-dependency-probe)
  add_subdirectory(pf-visual-audio-probe)
  add_subdirectory(pf-path-param-probe)
  add_subdirectory(pf-arbitrary-scan-probe)
  add_subdirectory(pf-aegp-layer-receipt-probe)
  add_subdirectory(pf-aegp-layer-options-probe)
  add_subdirectory(pf-aegp-async-layer-receipt-probe)
  add_subdirectory(pf-aegp-render-suite5-probe)
  add_subdirectory(pf-aegp-render-options4-tail-probe)
  add_subdirectory(pf-aegp-platform-world-probe)
  add_subdirectory(pf-aegp-external-cache-roundtrip-probe)
  add_subdirectory(pf-aegp-owned-world-probe)
  add_subdirectory(pf-aegp-fast-blur-probe)
  add_subdirectory(pf-convolve-depth-probe)
  add_subdirectory(pf-transfer-rect-probe)
  add_subdirectory(pf-transform-affine-probe)
  add_subdirectory(pf-adv-time-probe)
else()
  message(STATUS "AE_SDK_ROOT is unavailable; pf-null-echo build is skipped")
endif()
