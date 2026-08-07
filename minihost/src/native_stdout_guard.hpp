#pragma once

namespace aexcompat::worker_runtime {

bool redirect_native_stdout();
void restore_native_stdout();

// Writes one marker through the redirected stdout and restores it, so a test
// can observe where a plug-in's own stdout would have gone: nowhere by
// default, stderr under AEXCOMPAT_EXTENDED_DIAG (issue #905). Returns false
// if the redirect could not be installed.
bool selftest_native_stdout_routing();

}  // namespace aexcompat::worker_runtime
