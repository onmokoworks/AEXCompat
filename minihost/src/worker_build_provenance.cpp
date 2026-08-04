// The worker's build provenance, as a string the broker can find by reading the
// executable's bytes (issue #649).
//
// The broker admits a locally built worker by comparing `minihost/src` mtimes
// against the worker's. A worker shipped beside the plugin has no repository
// around it, so that comparison has nothing to read and the worker is refused.
// Carrying the identity inside the binary is what survives leaving the build
// tree behind - and it survives being copied, which an mtime does not (CI's
// "Mirror workers into expected build layouts" step copies these executables).
//
// Read before launch, from the same bytes the broker hashes to pin the worker's
// identity, so recognizing it costs no extra read and no extra process - and
// cannot describe a different file than the one that was admitted.

#include "worker_build_provenance.hpp"

extern "C" {

// `extern` because a namespace-scope `const` has internal linkage even inside
// `extern "C"`, and the linker reference below needs a symbol to find. Nothing
// in the worker reads this; the explicit `/INCLUDE:` is what keeps `/OPT:REF`
// from dropping it out of a release build.
extern const char aexcompat_worker_build_provenance[];

// One contiguous literal, in a fixed shape: the broker finds it by searching the
// worker's bytes, so it must not be assembled at run time or split into pieces.
const char aexcompat_worker_build_provenance[] =
    "AEXCOMPAT-WORKER-PROVENANCE-V1"
    " rev=" AEXCOMPAT_WORKER_BUILD_REVISION
    " dirty=" AEXCOMPAT_WORKER_BUILD_DIRTY
    " END";

}  // extern "C"

#if defined(_MSC_VER)
#pragma comment(linker, "/INCLUDE:aexcompat_worker_build_provenance")
#endif
