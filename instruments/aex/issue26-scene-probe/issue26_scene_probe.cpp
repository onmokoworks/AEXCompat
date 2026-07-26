#include "AEConfig.h"
#include "entry.h"
#include "AE_GeneralPlug.h"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <sstream>
#include <string>
#include <vector>

namespace {

constexpr A_long kSchemaVersion = 1;
constexpr A_long kMaxItems = 128;
constexpr A_long kMaxLayers = 64;
constexpr A_long kMaxEffects = 32;
constexpr A_long kMaxStreams = 64;

SPBasicSuite* g_basic = nullptr;
AEGP_PluginID g_plugin_id = 0;
AEGP_Command g_command = 0;
bool g_written = false;
A_long g_driver_major = 0;
A_long g_driver_minor = 0;

struct UnsupportedSlot {
  std::string suite;
  A_long version{};
  A_long slot{};
  std::string operation;
  A_Err error{};
};

struct Identity {
  std::string kind;
  std::uint64_t token{};
  A_long stable_id{-1};
  A_long owner_id{-1};
  A_long ordinal{-1};
  A_long subtype{-1};
};

struct StreamObservation {
  std::uint64_t token{};
  std::uint64_t owner{};
  A_long ordinal{-1};
  A_long type{-1};
  A_long keyframes{-1};
};

struct Report {
  A_long driver_major{};
  A_long driver_minor{};
  std::vector<UnsupportedSlot> unsupported;
  std::vector<Identity> identities;
  std::vector<StreamObservation> streams;
  bool active_comp{};
  bool effect_order_observed{};
  bool effect_order_total{};
  bool stream_metadata_observed{};
  bool parent_observed{};
  bool camera_observed{};
  bool zoom_observed{};
  bool keyframe_interpolation_observed{};
  bool keyframe_ease_observed{};
  bool keyframe_tangents_observed{};
  bool transaction_cancel_observed{};
  bool transaction_cancel_unchanged{};
  bool transaction_commit_observed{};
  bool transaction_commit_incremented{};
  bool stale_owner_rejected{};
  A_long transaction_before{-1};
  A_long transaction_after_cancel{-1};
  A_long transaction_after_commit{-1};
  A_long suite_acquires{};
  A_long suite_releases{};
  A_long stream_acquires{};
  A_long stream_releases{};
  A_long effect_acquires{};
  A_long effect_releases{};
  A_long invalidated_children{};
  A_long applied_effects{};
  A_long removed_applied_effects{};
};

template <typename T>
struct SuiteLease {
  const char* name{};
  A_long version{};
  T* suite{};
  Report* report{};

  bool acquire(const char* suite_name, A_long suite_version, Report& out) {
    name = suite_name;
    version = suite_version;
    report = &out;
    const void* raw = nullptr;
    const A_Err error = g_basic && g_basic->AcquireSuite
        ? g_basic->AcquireSuite(name, version, &raw)
        : A_Err_GENERIC;
    if (error || !raw) {
      out.unsupported.push_back(
          {name, version, -1, "AcquireSuite", error ? error : A_Err_GENERIC});
      return false;
    }
    suite = const_cast<T*>(static_cast<const T*>(raw));
    ++out.suite_acquires;
    return true;
  }

  void release() {
    if (!suite || !g_basic || !g_basic->ReleaseSuite) return;
    if (g_basic->ReleaseSuite(name, version) == A_Err_NONE && report)
      ++report->suite_releases;
    suite = nullptr;
  }

  ~SuiteLease() { release(); }
};

std::uint64_t hash_bytes(const void* data, std::size_t size) {
  std::uint64_t hash = UINT64_C(14695981039346656037);
  const auto* bytes = static_cast<const unsigned char*>(data);
  for (std::size_t index = 0; index < size; ++index) {
    hash ^= bytes[index];
    hash *= UINT64_C(1099511628211);
  }
  return hash;
}

template <typename T>
std::uint64_t token_for(T handle) {
  return hash_bytes(&handle, sizeof(handle));
}

std::string escape_json(const std::string& value) {
  std::ostringstream output;
  for (unsigned char character : value) {
    switch (character) {
      case '"': output << "\\\""; break;
      case '\\': output << "\\\\"; break;
      case '\b': output << "\\b"; break;
      case '\f': output << "\\f"; break;
      case '\n': output << "\\n"; break;
      case '\r': output << "\\r"; break;
      case '\t': output << "\\t"; break;
      default:
        if (character < 0x20) {
          constexpr char digits[] = "0123456789abcdef";
          output << "\\u00" << digits[character >> 4] << digits[character & 15];
        } else {
          output << static_cast<char>(character);
        }
    }
  }
  return output.str();
}

const char* json_bool(bool value) { return value ? "true" : "false"; }

void note(Report& report, const char* suite, A_long version, A_long slot,
          const char* operation, A_Err error) {
  report.unsupported.push_back({suite, version, slot, operation, error});
}

void add_identity(Report& report, const char* kind, std::uint64_t token,
                  A_long stable_id, A_long owner_id, A_long ordinal,
                  A_long subtype) {
  report.identities.push_back(
      {kind, token, stable_id, owner_id, ordinal, subtype});
}

bool has_kind(const Report& report, const char* kind) {
  return std::any_of(
      report.identities.begin(), report.identities.end(),
      [kind](const Identity& value) { return value.kind == kind; });
}

std::wstring evidence_path() {
  std::array<wchar_t, 32768> path{};
  const DWORD count = GetEnvironmentVariableW(
      L"ISSUE26_SCENE_PROBE_EVIDENCE", path.data(),
      static_cast<DWORD>(path.size()));
  if (!count || count >= path.size()) return {};
  return std::wstring(path.data(), count);
}

std::string serialize(const Report& report) {
  const bool identity_coverage =
      has_kind(report, "project") && has_kind(report, "folder") &&
      has_kind(report, "footage") && has_kind(report, "comp") &&
      has_kind(report, "layer") && has_kind(report, "effect") &&
      !report.streams.empty();
  const bool cleanup_balanced =
      report.suite_acquires == report.suite_releases &&
      report.stream_acquires == report.stream_releases +
          report.invalidated_children &&
      report.effect_acquires == report.effect_releases &&
      report.applied_effects == report.removed_applied_effects;
  const bool behavioral_coverage =
      report.effect_order_observed && report.effect_order_total &&
      report.stream_metadata_observed && report.parent_observed &&
      report.camera_observed && report.zoom_observed &&
      report.keyframe_interpolation_observed &&
      report.keyframe_ease_observed &&
      report.keyframe_tangents_observed &&
      report.transaction_cancel_observed &&
      report.transaction_cancel_unchanged &&
      report.transaction_commit_observed &&
      report.transaction_commit_incremented &&
      report.stale_owner_rejected;
  const char* status =
      identity_coverage && behavioral_coverage && cleanup_balanced
          ? "passed"
          : report.active_comp ? "partial" : "blocked";

  std::ostringstream output;
  output << "{\"schema_version\":" << kSchemaVersion
         << ",\"probe\":\"issue26-public-aegp-scene\""
         << ",\"status\":\"" << status << "\""
         << ",\"host_input\":{\"driver_major\":" << report.driver_major
         << ",\"driver_minor\":" << report.driver_minor << "}"
         << ",\"oracle\":\"structural_identity_mutation_generation_stage_order_ownership\""
         << ",\"pixel_equality_required\":false"
         << ",\"identities\":[";
  for (std::size_t index = 0; index < report.identities.size(); ++index) {
    if (index) output << ',';
    const auto& value = report.identities[index];
    output << "{\"kind\":\"" << escape_json(value.kind)
           << "\",\"token\":\"" << std::hex << value.token << std::dec
           << "\",\"stable_id\":" << value.stable_id
           << ",\"owner_id\":" << value.owner_id
           << ",\"ordinal\":" << value.ordinal
           << ",\"subtype\":" << value.subtype << "}";
  }
  output << "],\"streams\":[";
  for (std::size_t index = 0; index < report.streams.size(); ++index) {
    if (index) output << ',';
    const auto& value = report.streams[index];
    output << "{\"token\":\"" << std::hex << value.token
           << "\",\"owner\":\"" << value.owner << std::dec
           << "\",\"ordinal\":" << value.ordinal
           << ",\"type\":" << value.type
           << ",\"keyframes\":" << value.keyframes << "}";
  }
  output << "],\"coverage\":{"
         << "\"identity_enumeration\":" << json_bool(identity_coverage)
         << ",\"effect_order\":{\"observed\":"
         << json_bool(report.effect_order_observed)
         << ",\"total\":" << json_bool(report.effect_order_total) << "}"
         << ",\"stream_metadata\":"
         << json_bool(report.stream_metadata_observed)
         << ",\"parent_camera_zoom\":{\"parent\":"
         << json_bool(report.parent_observed)
         << ",\"camera\":" << json_bool(report.camera_observed)
         << ",\"zoom\":" << json_bool(report.zoom_observed) << "}"
         << ",\"keyframes\":{\"interpolation\":"
         << json_bool(report.keyframe_interpolation_observed)
         << ",\"ease\":" << json_bool(report.keyframe_ease_observed)
         << ",\"spatial_tangents\":"
         << json_bool(report.keyframe_tangents_observed) << "}"
         << ",\"transaction\":{\"cancel_observed\":"
         << json_bool(report.transaction_cancel_observed)
         << ",\"cancel_unchanged\":"
         << json_bool(report.transaction_cancel_unchanged)
         << ",\"commit_observed\":"
         << json_bool(report.transaction_commit_observed)
         << ",\"commit_incremented\":"
         << json_bool(report.transaction_commit_incremented)
         << ",\"before\":" << report.transaction_before
         << ",\"after_cancel\":" << report.transaction_after_cancel
         << ",\"after_commit\":" << report.transaction_after_commit << "}"
         << ",\"generation\":{\"public_project_generation\":\"not_exposed\""
         << ",\"stale_owner_rejected\":"
         << json_bool(report.stale_owner_rejected) << "}"
         << "},\"unsupported_slots\":[";
  for (std::size_t index = 0; index < report.unsupported.size(); ++index) {
    if (index) output << ',';
    const auto& value = report.unsupported[index];
    output << "{\"suite\":\"" << escape_json(value.suite)
           << "\",\"version\":" << value.version
           << ",\"slot\":" << value.slot
           << ",\"operation\":\"" << escape_json(value.operation)
           << "\",\"error\":" << value.error << "}";
  }
  output << "],\"cleanup\":{\"suite_acquires\":" << report.suite_acquires
         << ",\"suite_releases\":" << report.suite_releases
         << ",\"stream_acquires\":" << report.stream_acquires
         << ",\"stream_releases\":" << report.stream_releases
         << ",\"effect_acquires\":" << report.effect_acquires
         << ",\"effect_releases\":" << report.effect_releases
         << ",\"invalidated_children\":" << report.invalidated_children
         << ",\"applied_effects\":" << report.applied_effects
         << ",\"removed_applied_effects\":"
         << report.removed_applied_effects
         << ",\"balanced\":" << json_bool(cleanup_balanced) << "}}\n";
  return output.str();
}

bool write_atomic(const std::wstring& path, const std::string& payload) {
  if (path.empty()) return false;
  const std::wstring temporary = path + L".tmp";
  FILE* file = nullptr;
  if (_wfopen_s(&file, temporary.c_str(), L"wb") != 0 || !file) return false;
  const bool written =
      std::fwrite(payload.data(), 1, payload.size(), file) == payload.size() &&
      std::fflush(file) == 0;
  const bool closed = std::fclose(file) == 0;
  if (!written || !closed) {
    DeleteFileW(temporary.c_str());
    return false;
  }
  if (!MoveFileExW(temporary.c_str(), path.c_str(),
                   MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
    DeleteFileW(temporary.c_str());
    return false;
  }
  return true;
}

void inspect_keyframes(Report& report, AEGP_StreamSuite6* streams,
                       AEGP_KeyframeSuite5* keyframes,
                       AEGP_StreamRefH stream) {
  if (!streams || !keyframes || !stream) return;
  A_long count = -1;
  if (keyframes->AEGP_GetStreamNumKFs(stream, &count) || count <= 0) return;
  AEGP_KeyframeInterpolationType in_interp{}, out_interp{};
  if (!keyframes->AEGP_GetKeyframeInterpolation(
          stream, 0, &in_interp, &out_interp))
    report.keyframe_interpolation_observed = true;
  AEGP_KeyframeEase in_ease{}, out_ease{};
  if (!keyframes->AEGP_GetKeyframeTemporalEase(
          stream, 0, 0, &in_ease, &out_ease))
    report.keyframe_ease_observed = true;
  AEGP_StreamValue2 in_tangent{}, out_tangent{};
  const A_Err tangent_error =
      keyframes->AEGP_GetNewKeyframeSpatialTangents(
          g_plugin_id, stream, 0, &in_tangent, &out_tangent);
  if (!tangent_error) {
    report.keyframe_tangents_observed = true;
    streams->AEGP_DisposeStreamValue(&in_tangent);
    streams->AEGP_DisposeStreamValue(&out_tangent);
  }
}

void exercise_transactions(Report& report, AEGP_StreamSuite6* streams,
                           AEGP_KeyframeSuite5* keyframes,
                           AEGP_StreamRefH stream) {
  if (!streams || !keyframes || !stream) return;
  A_long before = -1;
  if (keyframes->AEGP_GetStreamNumKFs(stream, &before)) return;
  report.transaction_before = before;
  const A_Time sample_time{0, 30};
  AEGP_StreamValue2 sample{};
  const A_Err sample_error = streams->AEGP_GetNewStreamValue(
      g_plugin_id, stream, AEGP_LTimeMode_CompTime, &sample_time, TRUE,
      &sample);
  if (sample_error) {
    note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 13,
         "AEGP_GetNewStreamValue(transaction)", sample_error);
    return;
  }

  const auto run = [&](A_Boolean commit, A_long frame,
                       A_long& after, bool& observed) {
    AEGP_AddKeyframesInfoH transaction = nullptr;
    A_Err error = keyframes->AEGP_StartAddKeyframes(stream, &transaction);
    A_long index = -1;
    const A_Time time{frame, 30};
    if (!error)
      error = keyframes->AEGP_AddKeyframes(
          transaction, AEGP_LTimeMode_CompTime, &time, &index);
    if (!error)
      error = keyframes->AEGP_SetAddKeyframe(transaction, index, &sample);
    if (transaction) {
      const A_Err terminal =
          keyframes->AEGP_EndAddKeyframes(error ? FALSE : commit, transaction);
      if (!error) error = terminal;
    }
    if (error) {
      note(report, kAEGPKeyframeSuite, kAEGPKeyframeSuiteVersion5,
           error && !transaction ? 16 : 19,
           commit ? "batch_commit" : "batch_cancel", error);
      return;
    }
    observed = true;
    keyframes->AEGP_GetStreamNumKFs(stream, &after);
  };

  run(FALSE, 7, report.transaction_after_cancel,
      report.transaction_cancel_observed);
  report.transaction_cancel_unchanged =
      report.transaction_cancel_observed &&
      report.transaction_after_cancel == report.transaction_before;
  run(TRUE, 8, report.transaction_after_commit,
      report.transaction_commit_observed);
  report.transaction_commit_incremented =
      report.transaction_commit_observed &&
      report.transaction_after_commit == report.transaction_before + 1;
  streams->AEGP_DisposeStreamValue(&sample);
}

void inspect_effects(Report& report, AEGP_LayerH layer, A_long layer_id,
                     AEGP_EffectSuite4* effects,
                     AEGP_StreamSuite6* streams,
                     AEGP_KeyframeSuite5* keyframes,
                     AEGP_EffectRefH& first_effect) {
  if (!effects || !streams || !layer) return;
  A_long count = -1;
  const A_Err count_error = effects->AEGP_GetLayerNumEffects(layer, &count);
  if (count_error) {
    note(report, kAEGPEffectSuite, kAEGPEffectSuiteVersion4, 0,
         "AEGP_GetLayerNumEffects", count_error);
    return;
  }
  report.effect_order_observed = true;
  report.effect_order_total = count >= 0 && count <= kMaxEffects;
  for (A_long index = 0; index < count && index < kMaxEffects; ++index) {
    AEGP_EffectRefH effect = nullptr;
    const A_Err effect_error = effects->AEGP_GetLayerEffectByIndex(
        g_plugin_id, layer, index, &effect);
    if (effect_error || !effect) {
      report.effect_order_total = false;
      note(report, kAEGPEffectSuite, kAEGPEffectSuiteVersion4, 1,
           "AEGP_GetLayerEffectByIndex", effect_error);
      continue;
    }
    ++report.effect_acquires;
    AEGP_InstalledEffectKey key = AEGP_InstalledEffectKey_NONE;
    const A_Err key_error =
        effects->AEGP_GetInstalledKeyFromLayerEffect(effect, &key);
    if (key_error) {
      note(report, kAEGPEffectSuite, kAEGPEffectSuiteVersion4, 2,
           "AEGP_GetInstalledKeyFromLayerEffect", key_error);
    }
    const auto effect_token = token_for(effect);
    add_identity(report, "effect", effect_token, static_cast<A_long>(key),
                 layer_id, index, 0);
    A_long stream_count = -1;
    const A_Err streams_error =
        streams->AEGP_GetEffectNumParamStreams(effect, &stream_count);
    if (streams_error) {
      note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 4,
           "AEGP_GetEffectNumParamStreams", streams_error);
    } else {
      bool metadata_complete = stream_count > 0;
      A_long streams_observed = 0;
      for (A_long stream_index = 0;
           stream_index < stream_count && stream_index < kMaxStreams;
           ++stream_index) {
        AEGP_StreamRefH stream = nullptr;
        const A_Err stream_error = streams->AEGP_GetNewEffectStreamByIndex(
            g_plugin_id, effect, stream_index, &stream);
        if (stream_error || !stream) {
          note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 5,
               "AEGP_GetNewEffectStreamByIndex", stream_error);
          continue;
        }
        ++report.stream_acquires;
        AEGP_StreamType type = AEGP_StreamType_NO_DATA;
        A_long key_count = -1;
        const A_Err type_error =
            streams->AEGP_GetStreamType(stream, &type);
        if (type_error)
          note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 12,
               "AEGP_GetStreamType", type_error);
        if (keyframes)
          keyframes->AEGP_GetStreamNumKFs(stream, &key_count);
        report.streams.push_back(
            {token_for(stream), effect_token, stream_index,
             static_cast<A_long>(type), key_count});
        metadata_complete = metadata_complete && !type_error;
        if (!type_error && stream_index == 0) {
          const A_Time zero_time{0, 30};
          AEGP_StreamValue2 input_value{};
          const A_Err value_error = streams->AEGP_GetNewStreamValue(
              g_plugin_id, stream, AEGP_LTimeMode_CompTime, &zero_time,
              TRUE, &input_value);
          metadata_complete = metadata_complete &&
              type == AEGP_StreamType_LAYER_ID && !value_error &&
              input_value.val.layer_id == layer_id;
          if (!value_error)
            streams->AEGP_DisposeStreamValue(&input_value);
          if (value_error)
            note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 13,
                 "AEGP_GetNewStreamValue(input layer)", value_error);
        }
        ++streams_observed;
        if (!streams->AEGP_DisposeStream(stream))
          ++report.stream_releases;
      }
      report.stream_metadata_observed =
          report.stream_metadata_observed ||
          (metadata_complete && streams_observed == stream_count);
    }
    if (!first_effect) {
      first_effect = effect;
    } else if (!effects->AEGP_DisposeEffect(effect)) {
      ++report.effect_releases;
    }
  }
}

bool ensure_effect_fixture(Report& report, AEGP_LayerH layer,
                           AEGP_EffectSuite4* effects) {
  if (!effects || !layer) return false;
  A_long count = -1;
  if (effects->AEGP_GetLayerNumEffects(layer, &count) || count < 0)
    return false;
  if (count != 0) return false;
  A_long installed_count = 0;
  AEGP_InstalledEffectKey key = AEGP_InstalledEffectKey_NONE;
  AEGP_EffectRefH applied = nullptr;
  A_Err error = effects->AEGP_GetNumInstalledEffects(&installed_count);
  if (!error && installed_count > 0)
    error = effects->AEGP_GetNextInstalledEffect(
        AEGP_InstalledEffectKey_NONE, &key);
  if (!error && key != AEGP_InstalledEffectKey_NONE)
    error = effects->AEGP_ApplyEffect(g_plugin_id, layer, key, &applied);
  if (error || !applied) {
    note(report, kAEGPEffectSuite, kAEGPEffectSuiteVersion4, 9,
         "AEGP_ApplyEffect(fixture)", error ? error : A_Err_GENERIC);
    return false;
  }
  ++report.effect_acquires;
  ++report.applied_effects;
  if (!effects->AEGP_DisposeEffect(applied))
    ++report.effect_releases;
  return true;
}

void exercise_stale_owner(Report& report, AEGP_EffectSuite4* effects,
                          AEGP_StreamSuite6* streams,
                          AEGP_EffectRefH first_effect) {
  if (!effects || !streams || !first_effect) return;
  AEGP_EffectRefH duplicate = nullptr;
  const A_Err duplicate_error =
      effects->AEGP_DuplicateEffect(first_effect, &duplicate);
  if (duplicate_error || !duplicate) {
    note(report, kAEGPEffectSuite, kAEGPEffectSuiteVersion4, 16,
         "AEGP_DuplicateEffect", duplicate_error);
    return;
  }
  ++report.effect_acquires;
  AEGP_StreamRefH child = nullptr;
  const A_Err child_error = streams->AEGP_GetNewEffectStreamByIndex(
      g_plugin_id, duplicate, 1, &child);
  if (child_error || !child) {
    note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 5,
         "AEGP_GetNewEffectStreamByIndex(stale)", child_error);
  } else {
    ++report.stream_acquires;
  }
  const A_Err delete_error = effects->AEGP_DeleteLayerEffect(duplicate);
  if (delete_error) {
    note(report, kAEGPEffectSuite, kAEGPEffectSuiteVersion4, 10,
         "AEGP_DeleteLayerEffect", delete_error);
  } else if (child) {
    AEGP_StreamType unchanged = static_cast<AEGP_StreamType>(0x1234);
    const A_Err stale_error = streams->AEGP_GetStreamType(child, &unchanged);
    report.stale_owner_rejected =
        stale_error != A_Err_NONE &&
        unchanged == static_cast<AEGP_StreamType>(0x1234);
    if (report.stale_owner_rejected) {
      ++report.invalidated_children;
    } else {
      note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 12,
           "stale_owner_rejection", stale_error);
      if (!streams->AEGP_DisposeStream(child))
        ++report.stream_releases;
    }
  }
  if (!delete_error) {
    ++report.effect_releases;
  } else if (!effects->AEGP_DisposeEffect(duplicate)) {
    ++report.effect_releases;
  }
}

void exercise_missing_parent_fixture(
    Report& report, AEGP_LayerSuite9* layers,
    const std::array<AEGP_LayerH, 2>& candidates) {
  if (report.parent_observed || !layers ||
      !candidates[0] || !candidates[1])
    return;
  const A_Err set_error =
      layers->AEGP_SetLayerParent(candidates[1], candidates[0]);
  if (set_error) {
    note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 42,
         "AEGP_SetLayerParent(fixture)", set_error);
    return;
  }
  AEGP_LayerH observed = nullptr;
  const A_Err get_error =
      layers->AEGP_GetLayerParent(candidates[1], &observed);
  report.parent_observed = !get_error && observed == candidates[0];
  const A_Err reset_error =
      layers->AEGP_SetLayerParent(candidates[1], nullptr);
  if (get_error)
    note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 41,
         "AEGP_GetLayerParent(fixture)", get_error);
  if (reset_error)
    note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 42,
         "AEGP_SetLayerParent(cleanup)", reset_error);
}

void exercise_missing_camera_fixture(
    Report& report, AEGP_CompH comp, AEGP_CompSuite11* comps,
    AEGP_LayerSuite9* layers, AEGP_StreamSuite6* streams) {
  if (report.camera_observed || !comp || !comps || !layers) return;
  const A_UTF16Char name[] = {
      'I', 's', 's', 'u', 'e', '2', '6', ' ', 'P', 'r', 'o', 'b', 'e', 0};
  const A_FloatPoint center{320.0F, 180.0F};
  AEGP_LayerH camera = nullptr;
  const A_Err create_error =
      comps->AEGP_CreateCameraInComp(name, center, comp, &camera);
  if (create_error || !camera) {
    note(report, kAEGPCompSuite, kAEGPCompSuiteVersion11, 23,
         "AEGP_CreateCameraInComp(fixture)",
         create_error ? create_error : A_Err_GENERIC);
    return;
  }
  AEGP_ObjectType type = AEGP_ObjectType_NONE;
  const A_Err type_error =
      layers->AEGP_GetLayerObjectType(camera, &type);
  report.camera_observed =
      !type_error && type == AEGP_ObjectType_CAMERA;
  if (type_error)
    note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 28,
         "AEGP_GetLayerObjectType(camera fixture)", type_error);
  if (streams) {
    AEGP_StreamRefH zoom = nullptr;
    const A_Err zoom_error = streams->AEGP_GetNewLayerStream(
        g_plugin_id, camera, AEGP_LayerStream_ZOOM, &zoom);
    if (!zoom_error && zoom) {
      ++report.stream_acquires;
      AEGP_StreamType stream_type = AEGP_StreamType_NO_DATA;
      report.zoom_observed =
          streams->AEGP_GetStreamType(zoom, &stream_type) == A_Err_NONE;
      if (!streams->AEGP_DisposeStream(zoom))
        ++report.stream_releases;
    } else {
      note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 3,
           "AEGP_GetNewLayerStream(camera fixture ZOOM)", zoom_error);
    }
  }
  const A_Err delete_error = layers->AEGP_DeleteLayer(camera);
  if (delete_error)
    note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 43,
         "AEGP_DeleteLayer(camera fixture)", delete_error);
}

Report observe(A_long driver_major, A_long driver_minor) {
  Report report{};
  report.driver_major = driver_major;
  report.driver_minor = driver_minor;

  SuiteLease<AEGP_ProjSuite6> projects;
  SuiteLease<AEGP_ItemSuite9> items;
  SuiteLease<AEGP_CompSuite11> comps;
  SuiteLease<AEGP_LayerSuite9> layers;
  SuiteLease<AEGP_EffectSuite4> effects;
  SuiteLease<AEGP_MaskSuite6> masks;
  SuiteLease<AEGP_StreamSuite6> streams;
  SuiteLease<AEGP_KeyframeSuite5> keyframes;
  const auto release_suites = [&] {
    keyframes.release();
    streams.release();
    masks.release();
    effects.release();
    layers.release();
    comps.release();
    items.release();
    projects.release();
  };

  projects.acquire(kAEGPProjSuite, kAEGPProjSuiteVersion6, report);
  if (!items.acquire(kAEGPItemSuite, kAEGPItemSuiteVersion9, report)) {
    release_suites();
    return report;
  }
  comps.acquire(kAEGPCompSuite, kAEGPCompSuiteVersion11, report);
  layers.acquire(kAEGPLayerSuite, kAEGPLayerSuiteVersion9, report);
  effects.acquire(kAEGPEffectSuite, kAEGPEffectSuiteVersion4, report);
  masks.acquire(kAEGPMaskSuite, kAEGPMaskSuiteVersion6, report);
  streams.acquire(kAEGPStreamSuite, kAEGPStreamSuiteVersion6, report);
  keyframes.acquire(kAEGPKeyframeSuite, kAEGPKeyframeSuiteVersion5, report);

  if (projects.suite) {
    A_long project_count = 0;
    const A_Err count_error =
        projects.suite->AEGP_GetNumProjects(&project_count);
    if (count_error) {
      note(report, kAEGPProjSuite, kAEGPProjSuiteVersion6, 0,
           "AEGP_GetNumProjects", count_error);
    }
    for (A_long project_index = 0;
         !count_error && project_index < project_count && project_index < 8;
         ++project_index) {
      AEGP_ProjectH project = nullptr;
      if (projects.suite->AEGP_GetProjectByIndex(project_index, &project) ||
          !project)
        continue;
      add_identity(report, "project", token_for(project), project_index, -1,
                   project_index, 0);
      AEGP_ItemH item = nullptr;
      A_Err item_error =
          items.suite->AEGP_GetFirstProjItem(project, &item);
      for (A_long ordinal = 0;
           !item_error && item && ordinal < kMaxItems; ++ordinal) {
        AEGP_ItemType type = AEGP_ItemType_NONE;
        A_long item_id = -1;
        if (!items.suite->AEGP_GetItemType(item, &type) &&
            !items.suite->AEGP_GetItemID(item, &item_id)) {
          const char* kind = type == AEGP_ItemType_FOLDER ? "folder"
              : type == AEGP_ItemType_COMP ? "comp"
              : type == AEGP_ItemType_FOOTAGE ? "footage" : "item";
          add_identity(report, kind, token_for(item), item_id, project_index,
                       ordinal, type);
        }
        AEGP_ItemH next = nullptr;
        item_error =
            items.suite->AEGP_GetNextProjItem(project, item, &next);
        item = next;
      }
      if (item_error)
        note(report, kAEGPItemSuite, kAEGPItemSuiteVersion9, 1,
             "AEGP_GetNextProjItem", item_error);
    }
  }

  AEGP_ItemH active_item = nullptr;
  AEGP_ItemType active_type = AEGP_ItemType_NONE;
  A_long active_item_id = -1;
  if (items.suite->AEGP_GetActiveItem(&active_item) || !active_item ||
      items.suite->AEGP_GetItemType(active_item, &active_type) ||
      active_type != AEGP_ItemType_COMP ||
      items.suite->AEGP_GetItemID(active_item, &active_item_id)) {
    release_suites();
    return report;
  }
  report.active_comp = true;
  if (!has_kind(report, "comp"))
    add_identity(report, "comp", token_for(active_item), active_item_id, -1,
                 0, active_type);
  if (!comps.suite || !layers.suite) {
    release_suites();
    return report;
  }

  AEGP_CompH comp = nullptr;
  const A_Err comp_error =
      comps.suite->AEGP_GetCompFromItem(active_item, &comp);
  if (comp_error || !comp) {
    note(report, kAEGPCompSuite, kAEGPCompSuiteVersion11, 0,
         "AEGP_GetCompFromItem", comp_error);
    release_suites();
    return report;
  }

  A_long layer_count = 0;
  const A_Err layer_count_error =
      layers.suite->AEGP_GetCompNumLayers(comp, &layer_count);
  if (layer_count_error) {
    note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 0,
         "AEGP_GetCompNumLayers", layer_count_error);
    release_suites();
    return report;
  }

  AEGP_EffectRefH first_effect = nullptr;
  AEGP_StreamRefH transaction_stream = nullptr;
  AEGP_MaskRefH transaction_mask = nullptr;
  bool applied_effect_fixture = false;
  AEGP_LayerH effect_fixture_layer = nullptr;
  A_long effect_fixture_layer_id = -1;
  AEGP_LayerH transaction_layer = nullptr;
  std::array<AEGP_LayerH, 2> parent_fixture_layers{};
  std::size_t parent_fixture_count = 0;
  bool saw_parent_candidate = false;
  const A_Time zero_time{0, 30};
  for (A_long index = 0;
       index < layer_count && index < kMaxLayers; ++index) {
    AEGP_LayerH layer = nullptr;
    if (layers.suite->AEGP_GetCompLayerByIndex(comp, index, &layer) ||
        !layer)
      continue;
    A_long layer_id = -1;
    layers.suite->AEGP_GetLayerID(layer, &layer_id);
    AEGP_ObjectType object_type = AEGP_ObjectType_NONE;
    layers.suite->AEGP_GetLayerObjectType(layer, &object_type);
    add_identity(report, "layer", token_for(layer), layer_id, active_item_id,
                 index, object_type);

    AEGP_ItemH source = nullptr;
    if (!layers.suite->AEGP_GetLayerSourceItem(layer, &source) && source) {
      AEGP_ItemType source_type = AEGP_ItemType_NONE;
      A_long source_id = -1;
      if (!items.suite->AEGP_GetItemType(source, &source_type) &&
          !items.suite->AEGP_GetItemID(source, &source_id)) {
        const char* kind = source_type == AEGP_ItemType_FOOTAGE
            ? "footage" : source_type == AEGP_ItemType_COMP ? "comp" : "item";
        if (std::none_of(report.identities.begin(), report.identities.end(),
                         [source_id](const Identity& value) {
                           return value.stable_id == source_id &&
                                  (value.kind == "footage" ||
                                   value.kind == "comp");
                         })) {
          add_identity(report, kind, token_for(source), source_id,
                       active_item_id, index, source_type);
        }
      }
    }

    AEGP_LayerH parent = nullptr;
    const A_Err parent_error =
        layers.suite->AEGP_GetLayerParent(layer, &parent);
    if (!parent_error) {
      saw_parent_candidate = true;
      report.parent_observed = report.parent_observed || parent != nullptr;
    } else {
      note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 41,
           "AEGP_GetLayerParent", parent_error);
    }
    A_Matrix4 matrix{};
    const A_Err matrix_error =
        layers.suite->AEGP_GetLayerToWorldXform(layer, &zero_time, &matrix);
    if (matrix_error)
      note(report, kAEGPLayerSuite, kAEGPLayerSuiteVersion9, 38,
           "AEGP_GetLayerToWorldXform", matrix_error);

    if (object_type == AEGP_ObjectType_CAMERA) {
      report.camera_observed = true;
      if (streams.suite) {
        AEGP_StreamRefH zoom = nullptr;
        const A_Err zoom_error = streams.suite->AEGP_GetNewLayerStream(
            g_plugin_id, layer, AEGP_LayerStream_ZOOM, &zoom);
        if (!zoom_error && zoom) {
          ++report.stream_acquires;
          AEGP_StreamType type = AEGP_StreamType_NO_DATA;
          report.zoom_observed =
              streams.suite->AEGP_GetStreamType(zoom, &type) == A_Err_NONE;
          if (!streams.suite->AEGP_DisposeStream(zoom))
            ++report.stream_releases;
        } else {
          note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 3,
               "AEGP_GetNewLayerStream(ZOOM)", zoom_error);
        }
      }
    }

    if (!transaction_layer && object_type != AEGP_ObjectType_CAMERA)
      transaction_layer = layer;
    if (object_type != AEGP_ObjectType_CAMERA &&
        parent_fixture_count < parent_fixture_layers.size())
      parent_fixture_layers[parent_fixture_count++] = layer;

    if (!effect_fixture_layer && object_type != AEGP_ObjectType_CAMERA) {
      effect_fixture_layer = layer;
      effect_fixture_layer_id = layer_id;
    }
    inspect_effects(report, layer, layer_id, effects.suite, streams.suite,
                    keyframes.suite, first_effect);
  }
  if (!saw_parent_candidate) report.parent_observed = false;
  exercise_missing_parent_fixture(
      report, layers.suite, parent_fixture_layers);
  exercise_missing_camera_fixture(
      report, comp, comps.suite, layers.suite, streams.suite);
  if (!first_effect && effect_fixture_layer && effects.suite) {
    applied_effect_fixture =
        ensure_effect_fixture(report, effect_fixture_layer, effects.suite);
    if (applied_effect_fixture)
      inspect_effects(report, effect_fixture_layer, effect_fixture_layer_id,
                      effects.suite, streams.suite, keyframes.suite,
                      first_effect);
  }
  if (masks.suite && streams.suite && transaction_layer) {
    A_long mask_count = 0;
    A_Err mask_error =
        masks.suite->AEGP_GetLayerNumMasks(transaction_layer, &mask_count);
    if (!mask_error && mask_count > 0)
      mask_error = masks.suite->AEGP_GetLayerMaskByIndex(
          transaction_layer, 0, &transaction_mask);
    if (!mask_error && transaction_mask)
      mask_error = streams.suite->AEGP_GetNewMaskStream(
          g_plugin_id, transaction_mask, AEGP_MaskStream_OUTLINE,
          &transaction_stream);
    if (!mask_error && transaction_stream) {
      ++report.stream_acquires;
    } else {
      note(report, kAEGPStreamSuite, kAEGPStreamSuiteVersion6, 6,
           "AEGP_GetNewMaskStream(OUTLINE)", mask_error);
    }
  }

  exercise_transactions(report, streams.suite, keyframes.suite,
                        transaction_stream);
  inspect_keyframes(report, streams.suite, keyframes.suite,
                    transaction_stream);
  if (transaction_stream &&
      !streams.suite->AEGP_DisposeStream(transaction_stream)) {
    ++report.stream_releases;
    transaction_stream = nullptr;
  }
  if (transaction_mask && !masks.suite->AEGP_DisposeMask(transaction_mask))
    transaction_mask = nullptr;
  exercise_stale_owner(report, effects.suite, streams.suite, first_effect);
  if (first_effect) {
    bool deleted = false;
    if (applied_effect_fixture &&
        !effects.suite->AEGP_DeleteLayerEffect(first_effect)) {
      ++report.removed_applied_effects;
      deleted = true;
    }
    if (deleted || !effects.suite->AEGP_DisposeEffect(first_effect))
      ++report.effect_releases;
  }
  release_suites();
  return report;
}

A_Err write_observation(A_long driver_major, A_long driver_minor) {
  if (g_written) return A_Err_NONE;
  const std::wstring path = evidence_path();
  if (path.empty()) return A_Err_NONE;
  Report report = observe(driver_major, driver_minor);
  // After Effects can invoke idle hooks before the JSX fixture has opened its
  // authored comp. Keep polling without publishing a premature blocker record.
  if (!report.active_comp) return A_Err_NONE;
  const bool written = write_atomic(path, serialize(report));
  if (written) g_written = true;
  return written ? A_Err_NONE : A_Err_GENERIC;
}

A_Err IdleHook(AEGP_GlobalRefcon, AEGP_IdleRefcon, A_long* max_sleep) {
  if (max_sleep) *max_sleep = 0;
  return write_observation(g_driver_major, g_driver_minor);
}

A_Err UpdateMenuHook(AEGP_GlobalRefcon, AEGP_UpdateMenuRefcon,
                     AEGP_WindowType) {
  return A_Err_NONE;
}

A_Err CommandHook(AEGP_GlobalRefcon, AEGP_CommandRefcon,
                  AEGP_Command command, AEGP_HookPriority,
                  A_Boolean, A_Boolean* handled) {
  if (handled) *handled = command == g_command ? TRUE : FALSE;
  return A_Err_NONE;
}

A_Err DeathHook(AEGP_GlobalRefcon, AEGP_DeathRefcon) {
  return A_Err_NONE;
}

}  // namespace

extern "C" DllExport AEGP_PluginInitFuncPrototype EntryPointFunc;

A_Err EntryPointFunc(SPBasicSuite* pica_basic, A_long major_version,
                     A_long minor_version, AEGP_PluginID plugin_id,
                     AEGP_GlobalRefcon* global_refcon) {
  if (!pica_basic || !global_refcon) return A_Err_GENERIC;
  g_basic = pica_basic;
  g_plugin_id = plugin_id;
  g_driver_major = major_version;
  g_driver_minor = minor_version;
  *global_refcon = reinterpret_cast<AEGP_GlobalRefcon>(&g_written);

  const AEGP_CommandSuite1* commands = nullptr;
  const AEGP_RegisterSuite5* registration = nullptr;
  A_Err error = pica_basic->AcquireSuite(
      kAEGPCommandSuite, kAEGPCommandSuiteVersion1,
      reinterpret_cast<const void**>(&commands));
  if (!error)
    error = pica_basic->AcquireSuite(
        kAEGPRegisterSuite, kAEGPRegisterSuiteVersion5,
        reinterpret_cast<const void**>(&registration));
  if (!error && (!commands || !registration)) error = A_Err_GENERIC;
  if (!error) error = commands->AEGP_GetUniqueCommand(&g_command);
  if (!error)
    error = commands->AEGP_InsertMenuCommand(
        g_command, "Issue26 Scene Probe", AEGP_Menu_EDIT,
        AEGP_MENU_INSERT_AT_BOTTOM);
  if (!error)
    error = registration->AEGP_RegisterCommandHook(
        plugin_id, AEGP_HP_BeforeAE, AEGP_Command_ALL, CommandHook, nullptr);
  if (!error)
    error = registration->AEGP_RegisterUpdateMenuHook(
        plugin_id, UpdateMenuHook, nullptr);
  if (!error)
    error = registration->AEGP_RegisterIdleHook(
        plugin_id, IdleHook, nullptr);
  if (!error)
    error = registration->AEGP_RegisterDeathHook(
        plugin_id, DeathHook, nullptr);

  if (registration)
    pica_basic->ReleaseSuite(
        kAEGPRegisterSuite, kAEGPRegisterSuiteVersion5);
  if (commands)
    pica_basic->ReleaseSuite(
        kAEGPCommandSuite, kAEGPCommandSuiteVersion1);
  return error;
}
