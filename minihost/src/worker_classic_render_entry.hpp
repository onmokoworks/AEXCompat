#pragma once

// The one declaration of the classic render entry point.
//
// `render_once` is defined in worker_classic_render_runtime.cpp and called from
// worker_invocation_orchestration.cpp (the one-shot routes) and
// worker_render_session.cpp (the resident session loop). Each of those used to
// carry its own hand-written copy of the 22-parameter signature, defaults and
// all, in its own `namespace aexcompat::l2_detail` block.
//
// That cost a correctness defect during issue #984: two out-parameters were
// added to all three copies, and the fourth edit - threading them into the
// struct the definition actually fills - was the one nothing forced. The
// parameters linked and were dropped on the floor inside the function. A single
// declaration makes the compiler the thing that notices, and it also removes
// the quieter hazard the copies carried: a default argument changed in one
// declaring translation unit and not the other renders one route with different
// time or pixel defaults than the other, from the same call spelling, with no
// diagnostic.

#include "render_subsystem.h"
#include "worker_parameter_execution.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_request_parser.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::l2_detail {

using EffectEntry = aexcompat::worker_runtime::parameter_execution::EffectEntry;
using RequestedAssignments = aexcompat::worker_runtime::parameters::RequestedAssignments;
using ExternalLayerInput = aexcompat::worker_runtime::request_parser::LayerInput;
// The frozen protocol buffers, from the one place that already owns their size
// rather than a fresh pair of literals: the same 408 is hand-written in a dozen
// translation units, and adding to that count in the header written to remove
// duplication would be the wrong direction.
using BufferIn = aexcompat::worker_runtime::parameter_execution::BufferIn;
using BufferOut = aexcompat::worker_runtime::parameter_execution::BufferOut;

// Runs one Classic frame: SEQUENCE/FRAME lifecycle, output resize negotiation,
// RENDER, and the host's own finalize. `width`, `height` and `rowbytes` are
// in/out - an accepted resize moves them to the extent the effect asked for.
int32_t render_once(EffectEntry entry, BufferIn& input,
                    BufferOut& output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes, std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr,
                    const std::vector<unsigned char>* external_rgba = nullptr,
                    int32_t external_width = 0, int32_t external_height = 0,
                    const std::vector<ExternalLayerInput>* external_layers = nullptr,
                    int32_t external_current_time = 0, int32_t external_time_step = 1,
                    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                    int32_t external_pixel_bytes = 4, bool manage_sequence = true,
                    std::vector<unsigned char>* captured_argb = nullptr,
                    aexcompat::render::ClassicFrameOutput* frame_output = nullptr);

}  // namespace aexcompat::l2_detail
