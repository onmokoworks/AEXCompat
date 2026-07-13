#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"

#include <cstddef>
#include <iostream>

namespace {
template <typename T>
void field(const char* name, std::size_t offset, bool& first) {
  if (!first) std::cout << ',';
  first = false;
  std::cout << "\n    \"" << name << "\":{\"offset\":" << offset
            << ",\"size\":" << sizeof(T) << '}';
}
}  // namespace

int main() {
  bool first = true;
  std::cout << "{\n  \"schema_version\":1,\n"
               "  \"sdk_boundary\":\"instrument_observation\",\n"
               "  \"pointer_size\":" << sizeof(void*) << ",\n"
               "  \"pf_in_data_size\":" << sizeof(PF_InData) << ",\n"
               "  \"pf_out_data_size\":" << sizeof(PF_OutData) << ",\n"
               "  \"pf_param_def_size\":" << sizeof(PF_ParamDef) << ",\n"
               "  \"pf_layer_def_size\":" << sizeof(PF_LayerDef) << ",\n"
               "  \"pf_interact_callbacks_size\":" << sizeof(PF_InteractCallbacks) << ",\n"
               "  \"pf_param_union_size\":" << sizeof(PF_ParamDefUnion) << ",\n"
               "  \"pf_util_callbacks_size\":" << sizeof(PF_UtilCallbacks) << ",\n"
               "  \"pf_pixel_size\":" << sizeof(PF_Pixel) << ",\n"
               "  \"pf_pre_render_extra_size\":" << sizeof(PF_PreRenderExtra) << ",\n"
               "  \"pf_pre_render_input_size\":" << sizeof(PF_PreRenderInput) << ",\n"
               "  \"pf_pre_render_output_size\":" << sizeof(PF_PreRenderOutput) << ",\n"
               "  \"pf_pre_render_callbacks_size\":" << sizeof(PF_PreRenderCallbacks) << ",\n"
               "  \"pf_smart_render_extra_size\":" << sizeof(PF_SmartRenderExtra) << ",\n"
               "  \"pf_smart_render_input_size\":" << sizeof(PF_SmartRenderInput) << ",\n"
               "  \"pf_smart_render_callbacks_size\":" << sizeof(PF_SmartRenderCallbacks) << ",\n"
               "  \"fields\":{";
  field<decltype(PF_InData::version)>("in.version", offsetof(PF_InData, version), first);
  field<decltype(PF_InData::serial_num)>("in.serial_num", offsetof(PF_InData, serial_num), first);
  field<decltype(PF_InData::appl_id)>("in.appl_id", offsetof(PF_InData, appl_id), first);
  field<decltype(PF_InData::num_params)>("in.num_params", offsetof(PF_InData, num_params), first);
  field<decltype(PF_InData::pica_basicP)>("in.pica_basicP", offsetof(PF_InData, pica_basicP), first);
  field<decltype(PF_InData::inter)>("in.inter", offsetof(PF_InData, inter), first);
  field<decltype(PF_InData::utils)>("in.utils", offsetof(PF_InData, utils), first);
  field<decltype(PF_InData::effect_ref)>("in.effect_ref", offsetof(PF_InData, effect_ref), first);
  field<decltype(PF_InData::global_data)>("in.global_data", offsetof(PF_InData, global_data), first);
  field<decltype(PF_InData::current_time)>("in.current_time", offsetof(PF_InData, current_time), first);
  field<decltype(PF_InData::time_step)>("in.time_step", offsetof(PF_InData, time_step), first);
  field<decltype(PF_InData::total_time)>("in.total_time", offsetof(PF_InData, total_time), first);
  field<decltype(PF_InData::time_scale)>("in.time_scale", offsetof(PF_InData, time_scale), first);
  field<decltype(PF_InData::width)>("in.width", offsetof(PF_InData, width), first);
  field<decltype(PF_InData::height)>("in.height", offsetof(PF_InData, height), first);
  field<decltype(PF_InteractCallbacks::checkout_param)>("inter.checkout_param", offsetof(PF_InteractCallbacks, checkout_param), first);
  field<decltype(PF_InteractCallbacks::checkin_param)>("inter.checkin_param", offsetof(PF_InteractCallbacks, checkin_param), first);
  field<decltype(PF_InteractCallbacks::add_param)>("inter.add_param", offsetof(PF_InteractCallbacks, add_param), first);
  field<decltype(PF_InteractCallbacks::abort)>("inter.abort", offsetof(PF_InteractCallbacks, abort), first);
  field<decltype(PF_InteractCallbacks::progress)>("inter.progress", offsetof(PF_InteractCallbacks, progress), first);
  field<decltype(PF_InteractCallbacks::register_ui)>("inter.register_ui", offsetof(PF_InteractCallbacks, register_ui), first);
  field<decltype(PF_UtilCallbacks::host_new_handle)>("utils.host_new_handle", offsetof(PF_UtilCallbacks, host_new_handle), first);
  field<decltype(PF_UtilCallbacks::host_lock_handle)>("utils.host_lock_handle", offsetof(PF_UtilCallbacks, host_lock_handle), first);
  field<decltype(PF_UtilCallbacks::host_unlock_handle)>("utils.host_unlock_handle", offsetof(PF_UtilCallbacks, host_unlock_handle), first);
  field<decltype(PF_UtilCallbacks::host_dispose_handle)>("utils.host_dispose_handle", offsetof(PF_UtilCallbacks, host_dispose_handle), first);
  field<decltype(PF_OutData::my_version)>("out.my_version", offsetof(PF_OutData, my_version), first);
  field<decltype(PF_OutData::global_data)>("out.global_data", offsetof(PF_OutData, global_data), first);
  field<decltype(PF_OutData::out_flags)>("out.out_flags", offsetof(PF_OutData, out_flags), first);
  field<decltype(PF_OutData::num_params)>("out.num_params", offsetof(PF_OutData, num_params), first);
  field<decltype(PF_OutData::return_msg)>("out.return_msg", offsetof(PF_OutData, return_msg), first);
  field<decltype(PF_OutData::out_flags2)>("out.out_flags2", offsetof(PF_OutData, out_flags2), first);
  field<decltype(PF_ParamDef::uu)>("param.uu", offsetof(PF_ParamDef, uu), first);
  field<decltype(PF_ParamDef::ui_flags)>("param.ui_flags", offsetof(PF_ParamDef, ui_flags), first);
  field<decltype(PF_ParamDef::ui_width)>("param.ui_width", offsetof(PF_ParamDef, ui_width), first);
  field<decltype(PF_ParamDef::ui_height)>("param.ui_height", offsetof(PF_ParamDef, ui_height), first);
  field<decltype(PF_ParamDef::param_type)>("param.param_type", offsetof(PF_ParamDef, param_type), first);
  field<decltype(PF_ParamDef::name)>("param.name", offsetof(PF_ParamDef, name), first);
  field<decltype(PF_ParamDef::flags)>("param.flags", offsetof(PF_ParamDef, flags), first);
  field<decltype(PF_ParamDef::u)>("param.u", offsetof(PF_ParamDef, u), first);
  field<decltype(PF_SliderDef::valid_min)>("slider.valid_min", offsetof(PF_SliderDef, valid_min), first);
  field<decltype(PF_SliderDef::valid_max)>("slider.valid_max", offsetof(PF_SliderDef, valid_max), first);
  field<decltype(PF_SliderDef::slider_min)>("slider.slider_min", offsetof(PF_SliderDef, slider_min), first);
  field<decltype(PF_SliderDef::slider_max)>("slider.slider_max", offsetof(PF_SliderDef, slider_max), first);
  field<decltype(PF_SliderDef::dephault)>("slider.default", offsetof(PF_SliderDef, dephault), first);
  field<decltype(PF_PopupDef::num_choices)>("popup.num_choices", offsetof(PF_PopupDef, num_choices), first);
  field<decltype(PF_PopupDef::dephault)>("popup.default", offsetof(PF_PopupDef, dephault), first);
  field<decltype(PF_PopupDef::u)>("popup.names", offsetof(PF_PopupDef, u), first);
  field<decltype(PF_CheckBoxDef::dephault)>("checkbox.default", offsetof(PF_CheckBoxDef, dephault), first);
  field<decltype(PF_CheckBoxDef::u)>("checkbox.label", offsetof(PF_CheckBoxDef, u), first);
  field<decltype(PF_FloatSliderDef::valid_min)>("float_slider.valid_min", offsetof(PF_FloatSliderDef, valid_min), first);
  field<decltype(PF_FloatSliderDef::valid_max)>("float_slider.valid_max", offsetof(PF_FloatSliderDef, valid_max), first);
  field<decltype(PF_FloatSliderDef::slider_min)>("float_slider.slider_min", offsetof(PF_FloatSliderDef, slider_min), first);
  field<decltype(PF_FloatSliderDef::slider_max)>("float_slider.slider_max", offsetof(PF_FloatSliderDef, slider_max), first);
  field<decltype(PF_FloatSliderDef::dephault)>("float_slider.default", offsetof(PF_FloatSliderDef, dephault), first);
  field<decltype(PF_FloatSliderDef::precision)>("float_slider.precision", offsetof(PF_FloatSliderDef, precision), first);
  field<decltype(PF_LayerDef::width)>("layer.width", offsetof(PF_LayerDef, width), first);
  field<decltype(PF_LayerDef::height)>("layer.height", offsetof(PF_LayerDef, height), first);
  field<decltype(PF_LayerDef::rowbytes)>("layer.rowbytes", offsetof(PF_LayerDef, rowbytes), first);
  field<decltype(PF_LayerDef::data)>("layer.data", offsetof(PF_LayerDef, data), first);
  field<decltype(PF_LayerDef::extent_hint)>("layer.extent_hint", offsetof(PF_LayerDef, extent_hint), first);
  field<decltype(PF_Pixel::alpha)>("pixel.alpha", offsetof(PF_Pixel, alpha), first);
  field<decltype(PF_Pixel::red)>("pixel.red", offsetof(PF_Pixel, red), first);
  field<decltype(PF_Pixel::green)>("pixel.green", offsetof(PF_Pixel, green), first);
  field<decltype(PF_Pixel::blue)>("pixel.blue", offsetof(PF_Pixel, blue), first);
  field<decltype(PF_PreRenderExtra::input)>("pre_extra.input", offsetof(PF_PreRenderExtra, input), first);
  field<decltype(PF_PreRenderExtra::output)>("pre_extra.output", offsetof(PF_PreRenderExtra, output), first);
  field<decltype(PF_PreRenderExtra::cb)>("pre_extra.callbacks", offsetof(PF_PreRenderExtra, cb), first);
  field<decltype(PF_PreRenderInput::output_request)>("pre_input.output_request", offsetof(PF_PreRenderInput, output_request), first);
  field<decltype(PF_PreRenderOutput::result_rect)>("pre_output.result_rect", offsetof(PF_PreRenderOutput, result_rect), first);
  field<decltype(PF_PreRenderOutput::max_result_rect)>("pre_output.max_result_rect", offsetof(PF_PreRenderOutput, max_result_rect), first);
  field<decltype(PF_PreRenderCallbacks::checkout_layer)>("pre_callbacks.checkout_layer", offsetof(PF_PreRenderCallbacks, checkout_layer), first);
  field<decltype(PF_SmartRenderExtra::input)>("smart_extra.input", offsetof(PF_SmartRenderExtra, input), first);
  field<decltype(PF_SmartRenderExtra::cb)>("smart_extra.callbacks", offsetof(PF_SmartRenderExtra, cb), first);
  field<decltype(PF_SmartRenderCallbacks::checkout_layer_pixels)>("smart_callbacks.checkout_layer_pixels", offsetof(PF_SmartRenderCallbacks, checkout_layer_pixels), first);
  field<decltype(PF_SmartRenderCallbacks::checkin_layer_pixels)>("smart_callbacks.checkin_layer_pixels", offsetof(PF_SmartRenderCallbacks, checkin_layer_pixels), first);
  field<decltype(PF_SmartRenderCallbacks::checkout_output)>("smart_callbacks.checkout_output", offsetof(PF_SmartRenderCallbacks, checkout_output), first);
  std::cout << "\n  },\n  \"selectors\":{"
            << "\"about\":" << static_cast<int>(PF_Cmd_ABOUT) << ','
            << "\"global_setup\":" << static_cast<int>(PF_Cmd_GLOBAL_SETUP) << ','
            << "\"global_setdown\":" << static_cast<int>(PF_Cmd_GLOBAL_SETDOWN) << ','
            << "\"params_setup\":" << static_cast<int>(PF_Cmd_PARAMS_SETUP)
            << ",\"render\":" << static_cast<int>(PF_Cmd_RENDER)
            << ",\"smart_pre_render\":" << static_cast<int>(PF_Cmd_SMART_PRE_RENDER)
            << ",\"smart_render\":" << static_cast<int>(PF_Cmd_SMART_RENDER)
            << "}\n}\n";
}
