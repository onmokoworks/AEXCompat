#pragma once

// Public executable identity is supplied by a deliberately tiny adapter.
// Keeping it out of compiler definitions lets all worker binaries link one
// macro-neutral runtime object graph.
namespace aexcompat::worker_target {

enum class Kind { L2, Render, Smart };

int run(Kind kind, int argc, wchar_t** argv);

}  // namespace aexcompat::worker_target
