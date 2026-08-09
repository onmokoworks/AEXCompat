#include "worker_aegp_persistent_data_suite.hpp"

#include "worker_extended_diag.hpp"
#include "worker_handle_runtime.hpp"

#include <windows.h>

#include <shlobj.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <iostream>
#include <iterator>
#include <mutex>
#include <string>
#include <utility>
#include <vector>

namespace aexcompat::l2_detail {
int32_t __cdecl acquire_suite(const char*, int32_t, const void**);
int32_t __cdecl release_suite(const char*, int32_t);
}  // namespace aexcompat::l2_detail

namespace aexcompat::worker_runtime::persistent_data {
namespace {

struct Entry {
  std::string key;
  ValueKind kind{ValueKind::data};
  std::vector<unsigned char> bytes;
};

struct Section {
  std::string key;
  std::vector<Entry> entries;
};

struct Blob {
  std::mutex mutex;
  std::vector<Section> sections;
  std::size_t stored_bytes{};
  Telemetry telemetry;
};

std::array<Blob, kPersistentTypeCount>& blobs() {
  static std::array<Blob, kPersistentTypeCount> instances;
  return instances;
}

Blob& blob() {
  return blobs()[kMachineSpecific];
}

Blob* blob_for_handle(AEGP_PersistentBlobH handle) noexcept {
  for (Blob& candidate : blobs())
    if (handle == &candidate) return &candidate;
  return nullptr;
}

Blob& state_for_handle(AEGP_PersistentBlobH handle) noexcept {
  if (Blob* const found = blob_for_handle(handle)) return *found;
  return blob();
}

bool valid_blob(AEGP_PersistentBlobH handle) noexcept {
  return blob_for_handle(handle) != nullptr;
}

// Plug-in memory, so every read of it is guarded: a caller may pass an
// unterminated or unmapped pointer, and the worker turns that into a refused
// call instead of an access violation. The copy is bounded first and validated
// afterwards, like the suite registry's own name copy.
bool copy_c_string_seh(const A_char* source, char* output, std::size_t bound,
                       std::size_t& length) noexcept {
  if (!source || !output) return false;
  __try {
    for (std::size_t index = 0; index <= bound; ++index) {
      const char value = source[index];
      if (value == '\0') {
        length = index;
        return true;
      }
      output[index] = value;
    }
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
  // Ran past the bound without a terminator: too long to be accepted here.
  return false;
}

// The staging buffer the bounded copy above writes into. It is reused rather
// than allocated per call: at `kMaxValueBytes` it is a megabyte, and a plug-in
// may read a string value once per parameter per frame. Thread-local because
// the suite is reachable from whatever thread the plug-in calls on, and this
// buffer is held across the SEH copy without the blob mutex.
std::vector<char>& read_staging(std::size_t bound) {
  static thread_local std::vector<char> buffer;
  if (buffer.size() < bound + 1) buffer.resize(bound + 1);
  return buffer;
}

// Reads one plug-in-supplied, NUL-terminated string of at most `bound` bytes.
// Returns false for an unreadable, unterminated or over-long one, which every
// entry point turns into A_Err_PARAMETER.
bool read_c_string(const A_char* source, std::size_t bound,
                   std::string& output) {
  std::size_t length{};
  try {
    std::vector<char>& buffer = read_staging(bound);
    if (!copy_c_string_seh(source, buffer.data(), bound, length)) return false;
    output.assign(buffer.data(), length);
    return true;
  } catch (...) {
    return false;
  }
}

bool copy_bytes_seh(const void* source, std::size_t size,
                    unsigned char* destination) noexcept {
  if (size == 0) return true;
  if (!source || !destination) return false;
  __try {
    std::memcpy(destination, source, size);
    return true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

template <typename T>
bool store_seh(T* destination, const T& value) noexcept {
  if (!destination) return false;
  __try {
    *destination = value;
    return true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

bool write_bytes_seh(void* destination, const void* source,
                     std::size_t size) noexcept {
  if (size == 0) return true;
  if (!destination || !source) return false;
  __try {
    std::memcpy(destination, source, size);
    return true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

// A key is rejected for shape, not for character set: AE's preferences hold
// names this host never sees the encoding rules for, so only what cannot be a
// key is refused - empty, over the bound (already handled by the copy), or
// carrying a control byte that would make a recorded name unreadable.
bool read_key(const A_char* source, std::string& output) {
  if (!read_c_string(source, kMaxKeyBytes, output)) return false;
  if (output.empty()) return false;
  return std::none_of(output.begin(), output.end(), [](char character) {
    const auto value = static_cast<unsigned char>(character);
    return value < 0x20 || value == 0x7f;
  });
}

Section* find_section(Blob& state, const std::string& key) noexcept {
  for (Section& section : state.sections)
    if (section.key == key) return &section;
  return nullptr;
}

Entry* find_entry(Section& section, const std::string& key) noexcept {
  for (Entry& entry : section.entries)
    if (entry.key == key) return &entry;
  return nullptr;
}

Entry* find_entry(Blob& state, const std::string& section_key,
                  const std::string& value_key) noexcept {
  Section* const section = find_section(state, section_key);
  return section ? find_entry(*section, value_key) : nullptr;
}

void recount(Blob& state) noexcept {
  state.telemetry.sections = static_cast<uint32_t>(state.sections.size());
  std::size_t keys = 0;
  for (const Section& section : state.sections) keys += section.entries.size();
  state.telemetry.keys = static_cast<uint32_t>(keys);
  state.telemetry.stored_bytes = state.stored_bytes;
}

// What one entry costs against `kMaxBlobBytes`: its own name plus its value.
// Keys count because a plug-in chooses them too, and 262144 entries of
// 255-byte keys is not a blob this host should hold just because every value
// is empty.
std::size_t entry_bytes(const Entry& entry) noexcept {
  return entry.key.size() + entry.bytes.size();
}

// Writes one value, replacing whatever was there.
//
// Every bound is checked and every allocation that can throw is done before
// anything in the blob is touched, so a refused or failed write leaves the
// blob exactly as it was. Getting this wrong is not a lost write: a half-
// applied store would leave an empty section that `AEGP_DeleteEntry` can never
// reach (it only prunes a section it removed an entry from), or a
// `stored_bytes` total above the real one, which would shrink the effective
// ceiling for the rest of the process.
A_Err store(Blob& state, const std::string& section_key,
            const std::string& value_key, ValueKind kind,
            const unsigned char* bytes, std::size_t size) {
  if (size > kMaxValueBytes) return kErrAlloc;
  Section* section = find_section(state, section_key);
  Entry* const entry = section ? find_entry(*section, value_key) : nullptr;
  const std::size_t previous = entry ? entry_bytes(*entry) : 0;
  const std::size_t added = value_key.size() + size +
      (section ? 0 : section_key.size());
  if (!section && state.sections.size() >= kMaxSections) return kErrAlloc;
  if (!entry && section && section->entries.size() >= kMaxKeysPerSection)
    return kErrAlloc;
  if (added > kMaxBlobBytes ||
      state.stored_bytes - previous > kMaxBlobBytes - added)
    return kErrAlloc;

  std::vector<unsigned char> value;
  try {
    value.assign(bytes, bytes + size);
  } catch (...) {
    return kErrAlloc;
  }

  if (entry) {
    // Existing entry: every step from here is noexcept.
    entry->kind = kind;
    entry->bytes = std::move(value);
    state.stored_bytes = state.stored_bytes - previous + added;
    recount(state);
    return kErrNone;
  }
  const bool created_section = section == nullptr;
  try {
    if (created_section) {
      state.sections.push_back(Section{section_key, {}});
      section = &state.sections.back();
    }
    try {
      section->entries.push_back(Entry{value_key, kind, std::move(value)});
    } catch (...) {
      // `push_back` leaves the vector unchanged when the element throws, so
      // undoing the section this call added is the whole rollback.
      if (created_section) state.sections.pop_back();
      throw;
    }
  } catch (...) {
    return kErrAlloc;
  }
  state.stored_bytes += added;
  recount(state);
  return kErrNone;
}

// The SDK's documented getter contract: a key that is not found gets the
// caller's default written into the blob and returned. A key that is found
// under a different setter's kind is the case AE's text blob does not have an
// observable answer for, so it is counted and answered with the default
// without rewriting what is stored.
enum class Lookup { hit, absent, kind_mismatch };

Lookup lookup(Blob& state, const std::string& section_key,
              const std::string& value_key, ValueKind kind,
              const Entry** found) noexcept {
  const Entry* const entry = find_entry(state, section_key, value_key);
  if (!entry) return Lookup::absent;
  if (entry->kind != kind) {
    ++state.telemetry.kind_mismatches;
    if (aexcompat::l2_detail::extended_diag_enabled())
      std::cerr << "extended_diag:persistent_data kind_mismatch stored="
                << static_cast<int>(entry->kind)
                << " requested=" << static_cast<int>(kind) << "\n"
                << std::flush;
    return Lookup::kind_mismatch;
  }
  *found = entry;
  return Lookup::hit;
}

// The directory a plug-in is handed for its own preference files.
//
// AE answers with its preferences folder. This host is not AE and must not
// hand a plug-in AE's own preferences tree, where a write would land next to
// (or on top of) the application's real settings, so it answers with a
// directory of its own, created on demand so the path is one the caller can
// actually write to.
//
// Resolved through SHGetKnownFolderPath rather than %APPDATA%: the plug-in
// shares this process, so it can rewrite that environment variable and choose
// where the host creates a directory. The known-folder API reads the user's
// profile instead of a value the plug-in can reach.
//
// Whether the trailing separator belongs in the answer is unverified. AE's own
// callers are assumed to append a leaf to what they get back, which is why it
// is included, but no observation of AE confirms it and the SDK header says
// only "empty string if no file". Tracked as an AE-oracle question rather than
// stated as fact (issue #886).
std::wstring resolve_prefs_directory() {
  PWSTR roaming = nullptr;
  if (FAILED(SHGetKnownFolderPath(FOLDERID_RoamingAppData, KF_FLAG_CREATE,
                                  nullptr, &roaming)) ||
      !roaming) {
    if (roaming) CoTaskMemFree(roaming);
    return std::wstring();
  }
  std::wstring path;
  try {
    path.assign(roaming);
  } catch (...) {
    CoTaskMemFree(roaming);
    return std::wstring();
  }
  CoTaskMemFree(roaming);
  while (!path.empty() && (path.back() == L'\\' || path.back() == L'/'))
    path.pop_back();
  if (path.empty()) return std::wstring();
  try {
    path += L"\\AEXCompat";
    if (!CreateDirectoryW(path.c_str(), nullptr) &&
        GetLastError() != ERROR_ALREADY_EXISTS)
      return std::wstring();
    path += L"\\Preferences";
    if (!CreateDirectoryW(path.c_str(), nullptr) &&
        GetLastError() != ERROR_ALREADY_EXISTS)
      return std::wstring();
    path += L'\\';
  } catch (...) {
    return std::wstring();
  }
  return path;
}

// Resolved once. The directory does not move for the life of the worker, and
// this keeps the filesystem calls out of the blob mutex and off the per-call
// path. An empty result is cached too: a profile this host cannot resolve or
// create under will not start working later in the same process.
const std::wstring& prefs_directory() {
  static const std::wstring path = resolve_prefs_directory();
  return path;
}

A_Err reject(Blob& state, A_Err error) noexcept {
  ++state.telemetry.rejected_calls;
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:persistent_data rejected error=" << error << "\n"
              << std::flush;
  return error;
}

// Every entry point below runs under the blob mutex: the suite is reachable
// from whatever thread the plug-in calls on, and the section/entry vectors are
// reallocated by writes.
A_Err __cdecl get_application_blob(AEGP_PersistentBlobH* handle) {
  Blob& state = blob();
  std::lock_guard<std::mutex> lock(state.mutex);
  if (!handle) return reject(state, kErrParameter);
  if (!store_seh<void*>(handle, &state))
    return reject(state, kErrParameter);
  return kErrNone;
}

A_Err __cdecl get_application_blob4(AEGP_PersistentType type,
                                    AEGP_PersistentBlobH* handle) {
  if (type < kMachineSpecific || type >= kPersistentTypeCount) {
    Blob& state = blob();
    std::lock_guard<std::mutex> lock(state.mutex);
    return reject(state, kErrParameter);
  }
  Blob& state = blobs()[static_cast<std::size_t>(type)];
  std::lock_guard<std::mutex> lock(state.mutex);
  if (!handle || !store_seh<void*>(handle, &state))
    return reject(state, kErrParameter);
  return kErrNone;
}

A_Err __cdecl get_num_sections(AEGP_PersistentBlobH handle, A_long* count) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  if (!valid_blob(handle) || !count) return reject(state, kErrParameter);
  if (!store_seh<A_long>(count, static_cast<A_long>(state.sections.size())))
    return reject(state, kErrParameter);
  return kErrNone;
}

// Shared by the two by-index readers. `max_size` is the caller's buffer, and a
// name that does not fit with its terminator is refused rather than truncated:
// a silently shortened section key would be a key the caller cannot look up.
A_Err copy_key_out(Blob& state, const std::string& key, A_long max_size,
                   A_char* destination) {
  if (max_size <= 0 || !destination) return reject(state, kErrParameter);
  if (key.size() + 1 > static_cast<std::size_t>(max_size))
    return reject(state, kErrParameter);
  if (!write_bytes_seh(destination, key.c_str(), key.size() + 1))
    return reject(state, kErrParameter);
  return kErrNone;
}

A_Err __cdecl get_section_key_by_index(AEGP_PersistentBlobH handle,
                                       A_long section_index, A_long max_size,
                                       A_char* section_key) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  if (!valid_blob(handle) || section_index < 0 ||
      static_cast<std::size_t>(section_index) >= state.sections.size())
    return reject(state, kErrParameter);
  return copy_key_out(state, state.sections[section_index].key, max_size,
                      section_key);
}

A_Err __cdecl does_key_exist(AEGP_PersistentBlobH handle,
                             const A_char* section_key,
                             const A_char* value_key, A_Boolean* exists) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string value;
  if (!valid_blob(handle) || !exists || !read_key(section_key, section) ||
      !read_key(value_key, value))
    return reject(state, kErrParameter);
  const A_Boolean present = find_entry(state, section, value) ? 1 : 0;
  if (!store_seh<A_Boolean>(exists, present))
    return reject(state, kErrParameter);
  return kErrNone;
}

A_Err __cdecl get_num_keys(AEGP_PersistentBlobH handle,
                           const A_char* section_key, A_long* count) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  if (!valid_blob(handle) || !count || !read_key(section_key, section))
    return reject(state, kErrParameter);
  // A section that was never written holds no keys. That is the same answer
  // enumeration gives after the section is deleted, so a caller walking an
  // unknown section gets an empty walk instead of an error.
  const Section* const found = find_section(state, section);
  const auto keys = static_cast<A_long>(found ? found->entries.size() : 0);
  if (!store_seh<A_long>(count, keys)) return reject(state, kErrParameter);
  return kErrNone;
}

A_Err __cdecl get_value_key_by_index(AEGP_PersistentBlobH handle,
                                     const A_char* section_key,
                                     A_long key_index, A_long max_size,
                                     A_char* value_key) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  if (!valid_blob(handle) || !read_key(section_key, section))
    return reject(state, kErrParameter);
  const Section* const found = find_section(state, section);
  if (!found || key_index < 0 ||
      static_cast<std::size_t>(key_index) >= found->entries.size())
    return reject(state, kErrParameter);
  return copy_key_out(state, found->entries[key_index].key, max_size,
                      value_key);
}

A_Err __cdecl get_data_handle(AEGP_PluginID, AEGP_PersistentBlobH handle,
                              const A_char* section_key,
                              const A_char* value_key,
                              AEGP_MemHandle default_handle,
                              AEGP_MemHandle* value) {
  // The plug-in id names the caller for AE's own accounting. Handles this host
  // allocates are owned by the worker's memory suite, which registers them
  // under its single host id, so the argument is not a second identity to
  // check against.
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  if (!valid_blob(handle) || !value || !read_key(section_key, section) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  ++state.telemetry.get_calls;

  std::vector<unsigned char> bytes;
  const Entry* entry = nullptr;
  const Lookup outcome = lookup(state, section, key, ValueKind::data, &entry);
  if (outcome == Lookup::hit) {
    try {
      bytes = entry->bytes;
    } catch (...) {
      return reject(state, kErrAlloc);
    }
  } else if (default_handle) {
    // Never adopted: the default is read through the memory suite and copied.
    uint32_t size = 0;
    if (handles::get_aegp_mem_handle_size(default_handle, &size) != 0)
      return reject(state, kErrParameter);
    if (size > kMaxValueBytes) return reject(state, kErrAlloc);
    void* data = nullptr;
    if (handles::lock_aegp_mem_handle(default_handle, &data) != 0)
      return reject(state, kErrParameter);
    bool copied = true;
    try {
      bytes.resize(size);
    } catch (...) {
      copied = false;
    }
    if (copied && size)
      copied = copy_bytes_seh(data, size, bytes.data());
    handles::unlock_aegp_mem_handle(default_handle);
    if (!copied) return reject(state, kErrAlloc);
  }

  if (outcome == Lookup::absent) {
    const A_Err stored =
        store(state, section, key, ValueKind::data, bytes.data(), bytes.size());
    if (stored != kErrNone) return reject(state, stored);
    ++state.telemetry.defaults_written;
  }

  // Documented: NULL for what would be a zero-sized handle.
  if (bytes.empty()) {
    if (!store_seh<void*>(value, nullptr)) return reject(state, kErrParameter);
    return kErrNone;
  }
  void* allocated = nullptr;
  if (handles::new_aegp_mem_handle(1, "persistent data value",
                                   static_cast<uint32_t>(bytes.size()), 1,
                                   &allocated) != 0)
    return reject(state, kErrAlloc);
  void* data = nullptr;
  if (handles::lock_aegp_mem_handle(allocated, &data) != 0) {
    handles::free_aegp_mem_handle(allocated);
    return reject(state, kErrAlloc);
  }
  std::memcpy(data, bytes.data(), bytes.size());
  handles::unlock_aegp_mem_handle(allocated);
  if (!store_seh<void*>(value, allocated)) {
    handles::free_aegp_mem_handle(allocated);
    return reject(state, kErrParameter);
  }
  return kErrNone;
}

A_Err __cdecl get_data(AEGP_PersistentBlobH handle, const A_char* section_key,
                       const A_char* value_key, A_u_long data_size,
                       const void* default_value, void* buffer) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  // A zero-length read is admitted, not refused: AEGP_SetData accepts a
  // zero-length write, so refusing the matching read would make a value this
  // suite can store one it cannot hand back. It writes nothing and still needs
  // a valid buffer.
  if (!valid_blob(handle) || !buffer || !read_key(section_key, section) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  if (data_size > kMaxValueBytes) return reject(state, kErrAlloc);
  ++state.telemetry.get_calls;

  std::vector<unsigned char> bytes;
  try {
    // Documented: a NULL default means all zeros.
    bytes.assign(data_size, 0);
  } catch (...) {
    return reject(state, kErrAlloc);
  }
  if (default_value && !copy_bytes_seh(default_value, data_size, bytes.data()))
    return reject(state, kErrParameter);

  const Entry* entry = nullptr;
  const Lookup outcome = lookup(state, section, key, ValueKind::data, &entry);
  // Documented: "bufPV & default must be this big, if pref isn't then the
  // default will be used". A stored value of a different length is therefore
  // answered with the default and left alone.
  if (outcome == Lookup::hit && entry->bytes.size() == data_size)
    std::memcpy(bytes.data(), entry->bytes.data(), data_size);
  else if (outcome == Lookup::absent) {
    const A_Err stored =
        store(state, section, key, ValueKind::data, bytes.data(), data_size);
    if (stored != kErrNone) return reject(state, stored);
    ++state.telemetry.defaults_written;
  }
  if (!write_bytes_seh(buffer, bytes.data(), data_size))
    return reject(state, kErrParameter);
  return kErrNone;
}

A_Err __cdecl get_string(AEGP_PersistentBlobH handle,
                         const A_char* section_key, const A_char* value_key,
                         const A_char* default_value, A_u_long buffer_size,
                         A_char* buffer, A_u_long* actual_size) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  if (!valid_blob(handle) || !buffer || !read_key(section_key, section) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  ++state.telemetry.get_calls;

  // Documented: a NULL default means '\0'. A default that is unreadable or
  // past the string bound is a refused call, not a truncated one.
  std::string text;
  if (default_value && !read_c_string(default_value, kMaxValueBytes, text))
    return reject(state, kErrParameter);

  const Entry* entry = nullptr;
  const Lookup outcome = lookup(state, section, key, ValueKind::string, &entry);
  if (outcome == Lookup::hit) {
    try {
      text.assign(reinterpret_cast<const char*>(entry->bytes.data()),
                  entry->bytes.size());
    } catch (...) {
      return reject(state, kErrAlloc);
    }
  } else if (outcome == Lookup::absent) {
    const A_Err stored = store(
        state, section, key, ValueKind::string,
        reinterpret_cast<const unsigned char*>(text.data()), text.size());
    if (stored != kErrNone) return reject(state, stored);
    ++state.telemetry.defaults_written;
  }

  const std::size_t needed = text.size() + 1;
  if (actual_size &&
      !store_seh<A_u_long>(actual_size, static_cast<A_u_long>(needed)))
    return reject(state, kErrParameter);
  if (needed > buffer_size) {
    // Documented: the buffer is emptied when it is too small, and the caller
    // that passed no `actual_buf_sizeLu0` asked to hear about the mismatch as
    // an error instead.
    const char empty = '\0';
    if (buffer_size != 0 && !write_bytes_seh(buffer, &empty, 1))
      return reject(state, kErrParameter);
    return actual_size ? kErrNone : reject(state, kErrParameter);
  }
  if (!write_bytes_seh(buffer, text.c_str(), needed))
    return reject(state, kErrParameter);
  return kErrNone;
}

// The two scalar getters share everything but their type, so they share the
// body: look up under their own kind, write the default through on a miss.
template <typename T, ValueKind Kind>
A_Err get_scalar(AEGP_PersistentBlobH handle, const A_char* section_key,
                 const A_char* value_key, T default_value, T* value) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  if (!valid_blob(handle) || !value || !read_key(section_key, section) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  ++state.telemetry.get_calls;

  T result = default_value;
  const Entry* entry = nullptr;
  const Lookup outcome = lookup(state, section, key, Kind, &entry);
  if (outcome == Lookup::hit && entry->bytes.size() == sizeof(T))
    std::memcpy(&result, entry->bytes.data(), sizeof(T));
  else if (outcome == Lookup::absent) {
    const A_Err stored =
        store(state, section, key, Kind,
              reinterpret_cast<const unsigned char*>(&result), sizeof(T));
    if (stored != kErrNone) return reject(state, stored);
    ++state.telemetry.defaults_written;
  }
  if (!store_seh<T>(value, result)) return reject(state, kErrParameter);
  return kErrNone;
}

A_Err __cdecl get_long(AEGP_PersistentBlobH handle, const A_char* section_key,
                       const A_char* value_key, A_long default_value,
                       A_long* value) {
  return get_scalar<A_long, ValueKind::integer>(handle, section_key, value_key,
                                                default_value, value);
}

A_Err __cdecl get_fp_long(AEGP_PersistentBlobH handle,
                          const A_char* section_key, const A_char* value_key,
                          A_FpLong default_value, A_FpLong* value) {
  return get_scalar<A_FpLong, ValueKind::floating>(handle, section_key,
                                                   value_key, default_value,
                                                   value);
}

A_Err __cdecl set_data_handle(AEGP_PersistentBlobH handle,
                              const A_char* section_key,
                              const A_char* value_key,
                              const AEGP_MemHandle value) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  if (!valid_blob(handle) || !value || !read_key(section_key, section) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  ++state.telemetry.set_calls;

  // Not adopted: the bytes are copied and the caller keeps its handle.
  uint32_t size = 0;
  if (handles::get_aegp_mem_handle_size(value, &size) != 0)
    return reject(state, kErrParameter);
  if (size > kMaxValueBytes) return reject(state, kErrAlloc);
  void* data = nullptr;
  if (handles::lock_aegp_mem_handle(value, &data) != 0)
    return reject(state, kErrParameter);
  std::vector<unsigned char> bytes;
  bool copied = true;
  try {
    bytes.resize(size);
  } catch (...) {
    copied = false;
  }
  if (copied && size) copied = copy_bytes_seh(data, size, bytes.data());
  handles::unlock_aegp_mem_handle(value);
  if (!copied) return reject(state, kErrAlloc);

  const A_Err stored =
      store(state, section, key, ValueKind::data, bytes.data(), bytes.size());
  return stored == kErrNone ? kErrNone : reject(state, stored);
}

A_Err __cdecl set_data(AEGP_PersistentBlobH handle, const A_char* section_key,
                       const A_char* value_key, A_u_long data_size,
                       const void* data) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  if (!valid_blob(handle) || (data_size != 0 && !data) ||
      !read_key(section_key, section) || !read_key(value_key, key))
    return reject(state, kErrParameter);
  // Over the value ceiling is A_Err_ALLOC, the same answer the handle form and
  // `store` give: it is a size this host will not hold, not a malformed call.
  if (data_size > kMaxValueBytes) return reject(state, kErrAlloc);
  ++state.telemetry.set_calls;

  std::vector<unsigned char> bytes;
  try {
    bytes.resize(data_size);
  } catch (...) {
    return reject(state, kErrAlloc);
  }
  if (!copy_bytes_seh(data, data_size, bytes.data()))
    return reject(state, kErrParameter);
  const A_Err stored =
      store(state, section, key, ValueKind::data, bytes.data(), bytes.size());
  return stored == kErrNone ? kErrNone : reject(state, stored);
}

A_Err __cdecl set_string(AEGP_PersistentBlobH handle,
                         const A_char* section_key, const A_char* value_key,
                         const A_char* text) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  std::string value;
  if (!valid_blob(handle) || !read_key(section_key, section) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  // A NULL string is an empty value, which is what the getter's NULL default
  // writes too.
  if (text && !read_c_string(text, kMaxValueBytes, value))
    return reject(state, kErrParameter);
  ++state.telemetry.set_calls;
  const A_Err stored =
      store(state, section, key, ValueKind::string,
            reinterpret_cast<const unsigned char*>(value.data()), value.size());
  return stored == kErrNone ? kErrNone : reject(state, stored);
}

template <typename T, ValueKind Kind>
A_Err set_scalar(AEGP_PersistentBlobH handle, const A_char* section_key,
                 const A_char* value_key, T value) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section;
  std::string key;
  if (!valid_blob(handle) || !read_key(section_key, section) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  ++state.telemetry.set_calls;
  const A_Err stored =
      store(state, section, key, Kind,
            reinterpret_cast<const unsigned char*>(&value), sizeof(T));
  return stored == kErrNone ? kErrNone : reject(state, stored);
}

A_Err __cdecl set_long(AEGP_PersistentBlobH handle, const A_char* section_key,
                       const A_char* value_key, A_long value) {
  return set_scalar<A_long, ValueKind::integer>(handle, section_key, value_key,
                                                value);
}

A_Err __cdecl set_fp_long(AEGP_PersistentBlobH handle,
                          const A_char* section_key, const A_char* value_key,
                          A_FpLong value) {
  return set_scalar<A_FpLong, ValueKind::floating>(handle, section_key,
                                                   value_key, value);
}

A_Err __cdecl delete_entry(AEGP_PersistentBlobH handle,
                           const A_char* section_key,
                           const A_char* value_key) {
  Blob& state = state_for_handle(handle);
  std::lock_guard<std::mutex> lock(state.mutex);
  std::string section_name;
  std::string key;
  if (!valid_blob(handle) || !read_key(section_key, section_name) ||
      !read_key(value_key, key))
    return reject(state, kErrParameter);
  ++state.telemetry.delete_calls;
  // Documented: no error if the entry is not found.
  Section* const section = find_section(state, section_name);
  if (!section) return kErrNone;
  const auto found = std::find_if(
      section->entries.begin(), section->entries.end(),
      [&key](const Entry& entry) { return entry.key == key; });
  if (found == section->entries.end()) return kErrNone;
  state.stored_bytes -= found->bytes.size();
  section->entries.erase(found);
  // An emptied section stops being enumerable, which matches a blob that was
  // never given the section at all.
  if (section->entries.empty()) {
    const auto section_found = std::find_if(
        state.sections.begin(), state.sections.end(),
        [&section_name](const Section& candidate) {
          return candidate.key == section_name;
        });
    if (section_found != state.sections.end())
      state.sections.erase(section_found);
  }
  recount(state);
  return kErrNone;
}

A_Err __cdecl get_prefs_directory(AEGP_MemHandle* path) {
  // Resolved outside the blob mutex; the first call does the filesystem work
  // and every later one reads the cached result.
  const std::wstring& directory = prefs_directory();
  std::u16string text;
  bool converted = true;
  try {
    static_assert(sizeof(wchar_t) == sizeof(char16_t));
    text.assign(reinterpret_cast<const char16_t*>(directory.c_str()),
                directory.size());
  } catch (...) {
    converted = false;
  }

  Blob& state = blob();
  std::lock_guard<std::mutex> lock(state.mutex);
  if (!path) return reject(state, kErrParameter);
  ++state.telemetry.prefs_directory_calls;
  if (!converted) return reject(state, kErrAlloc);
  // An empty answer is what the SDK documents for "no file", so it is not a
  // refusal - but it means a plug-in got nowhere to write, which is worth a
  // count of its own rather than looking like a normal call.
  if (text.empty()) ++state.telemetry.prefs_directory_unavailable;
  void* handle = nullptr;
  if (handles::make_utf16_handle(text, "persistent data prefs directory",
                                 &handle) != 0)
    return reject(state, kErrAlloc);
  if (!store_seh<void*>(path, handle)) {
    handles::free_aegp_mem_handle(handle);
    return reject(state, kErrParameter);
  }
  return kErrNone;
}

const Suite3 g_suite3{
    &get_application_blob, &get_num_sections,      &get_section_key_by_index,
    &does_key_exist,       &get_num_keys,          &get_value_key_by_index,
    &get_data_handle,      &get_data,              &get_string,
    &get_long,             &get_fp_long,           &set_data_handle,
    &set_data,             &set_string,            &set_long,
    &set_fp_long,          &delete_entry,          &get_prefs_directory};

const Suite4 g_suite4{
    &get_application_blob4, &get_num_sections,      &get_section_key_by_index,
    &does_key_exist,        &get_num_keys,          &get_value_key_by_index,
    &get_data_handle,       &get_data,              &get_string,
    &get_long,              &get_fp_long,           &set_data_handle,
    &set_data,              &set_string,            &set_long,
    &set_fp_long,           &delete_entry,          &get_prefs_directory};

}  // namespace

const void* provide_suite3(void*) noexcept { return &g_suite3; }
const void* provide_suite4(void*) noexcept { return &g_suite4; }

Telemetry telemetry() noexcept {
  Blob& state = blob();
  std::lock_guard<std::mutex> lock(state.mutex);
  return state.telemetry;
}

void reset_for_selftest() noexcept {
  for (Blob& state : blobs()) {
    std::lock_guard<std::mutex> lock(state.mutex);
    state.sections.clear();
    state.stored_bytes = 0;
    state.telemetry = Telemetry{};
  }
}

// Drives the published table the way a plug-in does, so the checks are over
// observable behavior (round-trips, the documented default write-through, the
// documented buffer-too-small answer, refusals) rather than over the storage
// behind it.
bool selftest() {
  reset_for_selftest();
  const Suite3& suite = g_suite3;

  AEGP_PersistentBlobH handle = nullptr;
  if (suite.AEGP_GetApplicationBlob(&handle) != kErrNone || !handle)
    return false;
  if (suite.AEGP_GetApplicationBlob(nullptr) != kErrParameter) return false;

  // A handle this host never handed out is refused rather than dereferenced.
  int foreign = 0;
  A_long count = -1;
  if (suite.AEGP_GetNumSections(static_cast<void*>(&foreign), &count) !=
      kErrParameter)
    return false;
  if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 0)
    return false;

  // A missing key answers with the caller's default and writes it through.
  A_long integer = 0;
  A_Boolean exists = 1;
  if (suite.AEGP_DoesKeyExist(handle, "sec", "num", &exists) != kErrNone ||
      exists != 0)
    return false;
  if (suite.AEGP_GetLong(handle, "sec", "num", 42, &integer) != kErrNone ||
      integer != 42)
    return false;
  if (suite.AEGP_DoesKeyExist(handle, "sec", "num", &exists) != kErrNone ||
      exists != 1)
    return false;
  if (suite.AEGP_SetLong(handle, "sec", "num", 7) != kErrNone) return false;
  if (suite.AEGP_GetLong(handle, "sec", "num", 42, &integer) != kErrNone ||
      integer != 7)
    return false;

  // Read under another setter's kind: the default comes back and what is
  // stored is left alone.
  A_FpLong floating = 0;
  if (suite.AEGP_GetFpLong(handle, "sec", "num", 1.5, &floating) != kErrNone ||
      floating != 1.5)
    return false;
  if (suite.AEGP_GetLong(handle, "sec", "num", 42, &integer) != kErrNone ||
      integer != 7)
    return false;
  if (suite.AEGP_SetFpLong(handle, "sec", "real", 0.25) != kErrNone)
    return false;
  if (suite.AEGP_GetFpLong(handle, "sec", "real", 9.5, &floating) != kErrNone ||
      floating != 0.25)
    return false;

  // Strings: round-trip, then the documented too-small-buffer answer.
  char text[16] = {};
  A_u_long actual = 0;
  if (suite.AEGP_SetString(handle, "sec", "text", "abc") != kErrNone)
    return false;
  if (suite.AEGP_GetString(handle, "sec", "text", nullptr, sizeof(text), text,
                           &actual) != kErrNone)
    return false;
  if (std::strcmp(text, "abc") != 0 || actual != 4) return false;
  char narrow[2] = {'x', 'x'};
  actual = 0;
  if (suite.AEGP_GetString(handle, "sec", "text", nullptr, sizeof(narrow),
                           narrow, &actual) != kErrNone)
    return false;
  if (narrow[0] != '\0' || actual != 4) return false;
  // No `actual_buf_sizeLu0`: the caller asked to hear about the mismatch.
  if (suite.AEGP_GetString(handle, "sec", "text", nullptr, sizeof(narrow),
                           narrow, nullptr) != kErrParameter)
    return false;

  // Opaque data: round-trip, then the documented "stored value is not this
  // big, so the default is used" answer.
  const unsigned char payload[4] = {1, 2, 3, 4};
  unsigned char read[4] = {};
  if (suite.AEGP_SetData(handle, "sec", "bytes", sizeof(payload), payload) !=
      kErrNone)
    return false;
  if (suite.AEGP_GetData(handle, "sec", "bytes", sizeof(read), nullptr, read) !=
          kErrNone ||
      std::memcmp(read, payload, sizeof(read)) != 0)
    return false;
  const unsigned char shorter_default[2] = {5, 6};
  unsigned char shorter[2] = {9, 9};
  if (suite.AEGP_GetData(handle, "sec", "bytes", sizeof(shorter),
                         shorter_default, shorter) != kErrNone ||
      std::memcmp(shorter, shorter_default, sizeof(shorter)) != 0)
    return false;

  // Handle form: the stored bytes come back in a caller-owned handle, and a
  // zero-sized value is the documented NULL.
  void* data_handle = nullptr;
  if (suite.AEGP_GetDataHandle(1, handle, "sec", "bytes", nullptr,
                               &data_handle) != kErrNone ||
      !data_handle)
    return false;
  uint32_t data_size = 0;
  void* data = nullptr;
  const bool handle_matches =
      handles::get_aegp_mem_handle_size(data_handle, &data_size) == 0 &&
      data_size == sizeof(payload) &&
      handles::lock_aegp_mem_handle(data_handle, &data) == 0 && data &&
      std::memcmp(data, payload, sizeof(payload)) == 0;
  handles::unlock_aegp_mem_handle(data_handle);
  handles::free_aegp_mem_handle(data_handle);
  if (!handle_matches) return false;
  void* empty_handle = &foreign;
  if (suite.AEGP_GetDataHandle(1, handle, "sec", "empty", nullptr,
                               &empty_handle) != kErrNone ||
      empty_handle != nullptr)
    return false;

  // A default handle is read and copied, never adopted: it is still the
  // caller's to lock and free after the call, and what lands in the blob is a
  // copy of its bytes.
  const unsigned char default_payload[3] = {7, 8, 9};
  void* default_handle = nullptr;
  if (handles::new_aegp_mem_handle(1, "selftest default",
                                   sizeof(default_payload), 1,
                                   &default_handle) != 0 ||
      !default_handle)
    return false;
  {
    void* default_data = nullptr;
    if (handles::lock_aegp_mem_handle(default_handle, &default_data) != 0 ||
        !default_data)
      return false;
    std::memcpy(default_data, default_payload, sizeof(default_payload));
    handles::unlock_aegp_mem_handle(default_handle);
  }
  void* defaulted = nullptr;
  if (suite.AEGP_GetDataHandle(1, handle, "sec", "from-default",
                               default_handle, &defaulted) != kErrNone ||
      !defaulted || defaulted == default_handle)
    return false;
  {
    uint32_t size = 0;
    void* data = nullptr;
    const bool matches =
        handles::get_aegp_mem_handle_size(defaulted, &size) == 0 &&
        size == sizeof(default_payload) &&
        handles::lock_aegp_mem_handle(defaulted, &data) == 0 && data &&
        std::memcmp(data, default_payload, sizeof(default_payload)) == 0;
    handles::unlock_aegp_mem_handle(defaulted);
    handles::free_aegp_mem_handle(defaulted);
    if (!matches) return false;
  }
  // Not adopted: the caller's handle is still live and still holds its bytes.
  {
    uint32_t size = 0;
    void* data = nullptr;
    const bool still_owned =
        handles::get_aegp_mem_handle_size(default_handle, &size) == 0 &&
        size == sizeof(default_payload) &&
        handles::lock_aegp_mem_handle(default_handle, &data) == 0 && data &&
        std::memcmp(data, default_payload, sizeof(default_payload)) == 0;
    handles::unlock_aegp_mem_handle(default_handle);
    if (!still_owned) return false;
  }
  // The handle setter takes the same route: bytes copied out, caller keeps its
  // handle.
  if (suite.AEGP_SetDataHandle(handle, "sec", "set-handle", default_handle) !=
      kErrNone)
    return false;
  {
    unsigned char echoed[3] = {};
    if (suite.AEGP_GetData(handle, "sec", "set-handle", sizeof(echoed), nullptr,
                           echoed) != kErrNone ||
        std::memcmp(echoed, default_payload, sizeof(default_payload)) != 0)
      return false;
  }
  if (handles::free_aegp_mem_handle(default_handle) != 0) return false;
  // A handle this host's memory suite never issued is refused, not read.
  if (suite.AEGP_SetDataHandle(handle, "sec", "foreign",
                               static_cast<void*>(&foreign)) != kErrParameter)
    return false;
  {
    void* rejected = nullptr;
    if (suite.AEGP_GetDataHandle(1, handle, "sec", "foreign-default",
                                 static_cast<void*>(&foreign),
                                 &rejected) != kErrParameter)
      return false;
  }

  // Enumeration walks what was written, in write order.
  if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 1)
    return false;
  char name[8] = {};
  if (suite.AEGP_GetSectionKeyByIndex(handle, 0, sizeof(name), name) !=
          kErrNone ||
      std::strcmp(name, "sec") != 0)
    return false;
  // A buffer that cannot hold the name with its terminator is refused, not
  // truncated, and so is an index past the end.
  if (suite.AEGP_GetSectionKeyByIndex(handle, 0, 3, name) != kErrParameter)
    return false;
  if (suite.AEGP_GetSectionKeyByIndex(handle, 1, sizeof(name), name) !=
      kErrParameter)
    return false;
  static constexpr const char* kWrittenKeys[] = {
      "num", "real", "text", "bytes", "empty", "from-default", "set-handle"};
  static constexpr A_long kWrittenKeyCount =
      static_cast<A_long>(std::size(kWrittenKeys));
  A_long keys = 0;
  if (suite.AEGP_GetNumKeys(handle, "sec", &keys) != kErrNone ||
      keys != kWrittenKeyCount)
    return false;
  if (suite.AEGP_GetNumKeys(handle, "absent", &keys) != kErrNone || keys != 0)
    return false;
  char key_name[16] = {};
  for (A_long index = 0; index < kWrittenKeyCount; ++index)
    if (suite.AEGP_GetValueKeyByIndex(handle, "sec", index, sizeof(key_name),
                                      key_name) != kErrNone ||
        std::strcmp(key_name, kWrittenKeys[index]) != 0)
      return false;
  if (suite.AEGP_GetValueKeyByIndex(handle, "sec", kWrittenKeyCount,
                                    sizeof(key_name), key_name) != kErrParameter)
    return false;

  // Sections enumerate in write order too, which one section could not show.
  if (suite.AEGP_SetLong(handle, "later", "n", 1) != kErrNone) return false;
  if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 2)
    return false;
  char second[8] = {};
  if (suite.AEGP_GetSectionKeyByIndex(handle, 1, sizeof(second), second) !=
          kErrNone ||
      std::strcmp(second, "later") != 0)
    return false;
  if (suite.AEGP_DeleteEntry(handle, "later", "n") != kErrNone) return false;
  if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 1)
    return false;

  // Deleting is not an error when the entry is absent, and a section that
  // loses its last key stops being enumerable.
  if (suite.AEGP_DeleteEntry(handle, "sec", "absent") != kErrNone) return false;
  if (suite.AEGP_DeleteEntry(handle, "absent", "num") != kErrNone) return false;
  for (const char* key : kWrittenKeys)
    if (suite.AEGP_DeleteEntry(handle, "sec", key) != kErrNone) return false;
  if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 0)
    return false;

  // Malformed keys are refused before anything is stored: empty, absent, over
  // the length bound, or carrying a control byte.
  if (suite.AEGP_SetLong(handle, "", "num", 1) != kErrParameter) return false;
  if (suite.AEGP_SetLong(handle, "sec", nullptr, 1) != kErrParameter)
    return false;
  if (suite.AEGP_SetLong(handle, "se\tc", "num", 1) != kErrParameter)
    return false;
  {
    std::string over_long(kMaxKeyBytes + 1, 'k');
    if (suite.AEGP_SetLong(handle, over_long.c_str(), "num", 1) != kErrParameter)
      return false;
    std::string at_bound(kMaxKeyBytes, 'k');
    if (suite.AEGP_SetLong(handle, at_bound.c_str(), "num", 1) != kErrNone)
      return false;
    if (suite.AEGP_DeleteEntry(handle, at_bound.c_str(), "num") != kErrNone)
      return false;
  }
  if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 0)
    return false;

  // An unreadable pointer is a refused call, not an access violation. The
  // guard page is never written to, only handed to the suite.
  {
    SYSTEM_INFO info{};
    GetSystemInfo(&info);
    auto* const guard = static_cast<char*>(
        VirtualAlloc(nullptr, info.dwPageSize, MEM_COMMIT | MEM_RESERVE,
                     PAGE_NOACCESS));
    if (!guard) return false;
    const bool refused =
        suite.AEGP_SetLong(handle, guard, "num", 1) == kErrParameter &&
        suite.AEGP_SetString(handle, "sec", "text", guard) == kErrParameter &&
        suite.AEGP_SetData(handle, "sec", "bytes", 4, guard) == kErrParameter &&
        suite.AEGP_GetLong(handle, "sec", "num", 1,
                           reinterpret_cast<A_long*>(guard)) == kErrParameter;
    VirtualFree(guard, 0, MEM_RELEASE);
    if (!refused) return false;
    // The unwritable-output case above still wrote the default through first,
    // which is the documented getter contract, so it leaves an entry behind.
    if (suite.AEGP_DeleteEntry(handle, "sec", "num") != kErrNone) return false;
    if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 0)
      return false;
  }

  // Bounds. Over the value ceiling is refused, and a refused write leaves the
  // blob exactly as it was - no empty section, no inflated byte total.
  {
    std::vector<unsigned char> over(kMaxValueBytes + 1, 0xab);
    if (suite.AEGP_SetData(handle, "big", "value",
                           static_cast<A_u_long>(over.size()),
                           over.data()) != kErrAlloc)
      return false;
    if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 0)
      return false;
  }
  {
    // Fill the blob to its ceiling one maximum-sized value at a time, then
    // check that the write which does not fit is refused and changes nothing.
    const std::vector<unsigned char> block(kMaxValueBytes, 0xcd);
    bool refused_once = false;
    for (std::size_t index = 0; index < kMaxBlobBytes / kMaxValueBytes + 1;
         ++index) {
      const std::string key = "v" + std::to_string(index);
      const A_Err error = suite.AEGP_SetData(
          handle, "fill", key.c_str(), static_cast<A_u_long>(block.size()),
          block.data());
      if (error == kErrAlloc) {
        refused_once = true;
        A_long filled = 0;
        if (suite.AEGP_GetNumKeys(handle, "fill", &filled) != kErrNone ||
            filled != static_cast<A_long>(index))
          return false;
        break;
      }
      if (error != kErrNone) return false;
    }
    if (!refused_once) return false;
    A_long filled = 0;
    if (suite.AEGP_GetNumKeys(handle, "fill", &filled) != kErrNone) return false;
    for (A_long index = 0; index < filled; ++index)
      if (suite.AEGP_DeleteEntry(handle, "fill",
                                 ("v" + std::to_string(index)).c_str()) !=
          kErrNone)
        return false;
    if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 0)
      return false;
  }
  {
    // The per-section key ceiling, and a section ceiling reached one section at
    // a time. Both refuse without leaving a half-written section behind.
    for (std::size_t index = 0; index < kMaxKeysPerSection; ++index)
      if (suite.AEGP_SetLong(handle, "keys", ("k" + std::to_string(index)).c_str(),
                             1) != kErrNone)
        return false;
    if (suite.AEGP_SetLong(handle, "keys", "overflow", 1) != kErrAlloc)
      return false;
    A_long stored_keys = 0;
    if (suite.AEGP_GetNumKeys(handle, "keys", &stored_keys) != kErrNone ||
        stored_keys != static_cast<A_long>(kMaxKeysPerSection))
      return false;
    // "keys" is already one section, so the ceiling is reached after
    // kMaxSections - 1 more.
    for (std::size_t index = 0; index + 1 < kMaxSections; ++index)
      if (suite.AEGP_SetLong(handle, ("s" + std::to_string(index)).c_str(), "n",
                             1) != kErrNone)
        return false;
    if (suite.AEGP_SetLong(handle, "overflow", "n", 1) != kErrAlloc) return false;
    if (suite.AEGP_GetNumSections(handle, &count) != kErrNone ||
        count != static_cast<A_long>(kMaxSections))
      return false;
    // A refused section leaves no empty section behind, which enumeration
    // would otherwise show as a section with no keys.
    for (A_long index = 0; index < count; ++index) {
      char section_name[kMaxKeyBytes + 1] = {};
      A_long section_keys = 0;
      if (suite.AEGP_GetSectionKeyByIndex(handle, index, sizeof(section_name),
                                          section_name) != kErrNone ||
          suite.AEGP_GetNumKeys(handle, section_name, &section_keys) !=
              kErrNone ||
          section_keys == 0)
        return false;
    }
  }

  // The two counters that record where this host can answer differently from
  // AE, checked before the reset clears them: the type mismatch above, and the
  // defaults the getters wrote through.
  {
    const Telemetry before_reset = telemetry();
    if (before_reset.kind_mismatches == 0 ||
        before_reset.defaults_written == 0 || before_reset.get_calls == 0 ||
        before_reset.set_calls == 0 || before_reset.delete_calls == 0)
      return false;
  }
  reset_for_selftest();
  if (suite.AEGP_GetNumSections(handle, &count) != kErrNone || count != 0)
    return false;

  void* prefs = nullptr;
  if (suite.AEGP_GetPrefsDirectory(&prefs) != kErrNone || !prefs) return false;
  if (handles::free_aegp_mem_handle(prefs) != 0) return false;
  if (suite.AEGP_GetPrefsDirectory(nullptr) != kErrParameter) return false;

  const Telemetry counters = telemetry();
  if (counters.rejected_calls == 0 || counters.prefs_directory_calls != 1 ||
      counters.sections != 0 || counters.keys != 0 ||
      counters.stored_bytes != 0)
    return false;
  // Every handle this run created was freed by it: the suite hands out
  // caller-owned handles and must not keep one alive behind the caller's back.
  if (!handles::aegp_memory_balanced()) return false;

  reset_for_selftest();
  return true;
}

bool selftest4() {
  reset_for_selftest();
  const void* suite3_raw = nullptr;
  const void* suite4_raw = nullptr;
  if (aexcompat::l2_detail::acquire_suite(kSuiteName, kSuiteVersion3,
                                         &suite3_raw) != kErrNone ||
      !suite3_raw)
    return false;
  const A_Err acquired4 = aexcompat::l2_detail::acquire_suite(
      kSuiteName, kSuiteVersion4, &suite4_raw);
  if (acquired4 != kErrNone || !suite4_raw || suite4_raw == suite3_raw) {
    if (acquired4 == kErrNone)
      aexcompat::l2_detail::release_suite(kSuiteName, kSuiteVersion4);
    aexcompat::l2_detail::release_suite(kSuiteName, kSuiteVersion3);
    return false;
  }

  const auto exercise = [&]() {
    const Suite3& suite3 = *static_cast<const Suite3*>(suite3_raw);
    const Suite4& suite4 = *static_cast<const Suite4*>(suite4_raw);
    std::array<AEGP_PersistentBlobH, kPersistentTypeCount> handles{};
    for (AEGP_PersistentType type = kMachineSpecific;
         type < kPersistentTypeCount; ++type) {
      if (suite4.AEGP_GetApplicationBlob(type, &handles[type]) != kErrNone ||
          !handles[type])
        return false;
      for (AEGP_PersistentType prior = kMachineSpecific; prior < type; ++prior)
        if (handles[type] == handles[prior]) return false;
    }
    AEGP_PersistentBlobH invalid = nullptr;
    if (suite4.AEGP_GetApplicationBlob(-1, &invalid) != kErrParameter ||
        invalid ||
        suite4.AEGP_GetApplicationBlob(kPersistentTypeCount, &invalid) !=
            kErrParameter ||
        suite4.AEGP_GetApplicationBlob(kMachineSpecific, nullptr) !=
            kErrParameter)
      return false;

    // Every public preference domain has an independent store. Driving the
    // last public enum value also prevents a four-domain partial implementation
    // from satisfying this test.
    for (AEGP_PersistentType type = kMachineSpecific;
         type < kPersistentTypeCount; ++type)
      if (suite4.AEGP_SetLong(handles[type], "domain", "value", type + 10) !=
          kErrNone)
        return false;
    for (AEGP_PersistentType type = kMachineSpecific;
         type < kPersistentTypeCount; ++type) {
      A_long value = 0;
      if (suite4.AEGP_GetLong(handles[type], "domain", "value", 0, &value) !=
              kErrNone ||
          value != type + 10)
        return false;
    }

    // v3 remains the machine-specific view used before v4 existed.
    AEGP_PersistentBlobH legacy = nullptr;
    return suite3.AEGP_GetApplicationBlob(&legacy) == kErrNone &&
        legacy == handles[kMachineSpecific];
  };

  const bool passed = exercise();
  const bool released4 =
      aexcompat::l2_detail::release_suite(kSuiteName, kSuiteVersion4) == kErrNone;
  const bool released3 =
      aexcompat::l2_detail::release_suite(kSuiteName, kSuiteVersion3) == kErrNone;
  return passed && released4 && released3;
}

}  // namespace aexcompat::worker_runtime::persistent_data
