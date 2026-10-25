#[derive(Default)]
struct UiDrawState {
    objects: BTreeMap<u64, (&'static str, u32)>,
    theme: BTreeMap<u16, [u16; 3]>,
    context: u64,
    picker: Option<[u32; 4]>,
}
fn ui_words(
    unicorn: &Unicorn<'_, GuestState>,
    pointer: u64,
    count: usize,
) -> Result<Vec<u32>, String> {
    let bytes = unicorn
        .mem_read_as_vec(pointer, count * 4)
        .map_err(|e| e.to_string())?;
    Ok(bytes
        .chunks_exact(4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .collect())
}
fn ui_float(unicorn: &Unicorn<'_, GuestState>, register: RegisterX86) -> Result<u32, String> {
    let bytes = unicorn.reg_read_long(register).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
}
fn ui_object(unicorn: &Unicorn<'_, GuestState>, id: u64, kind: &str) -> Result<(), String> {
    match unicorn.get_data().ui_session.draw.objects.get(&id) {
        Some((actual, count)) if *actual == kind && *count > 0 => Ok(()),
        _ => Err(format!("invalid/stale Drawbot {kind} object {id}")),
    }
}
fn ui_new_object(
    unicorn: &mut Unicorn<'_, GuestState>,
    kind: &'static str,
    output: u64,
) -> Result<u64, String> {
    let id = ui_new_handle(unicorn)?;
    ui_write(unicorn, output, &id.to_le_bytes())?;
    unicorn
        .get_data_mut()
        .ui_session
        .draw
        .objects
        .insert(id, (kind, 1));
    Ok(id)
}
fn emulate_ui_draw_call(
    unicorn: &mut Unicorn<'_, GuestState>,
    suite: u64,
    slot: u64,
) -> Result<(), String> {
    use serde_json::json;
    let a = unicorn
        .reg_read(RegisterX86::RCX)
        .map_err(|e| e.to_string())?;
    let b = unicorn
        .reg_read(RegisterX86::RDX)
        .map_err(|e| e.to_string())?;
    let c = unicorn
        .reg_read(RegisterX86::R8)
        .map_err(|e| e.to_string())?;
    let d = unicorn
        .reg_read(RegisterX86::R9)
        .map_err(|e| e.to_string())?;
    let call = match (suite, slot) {
        (6, 0) if a == unicorn.get_data().ui_session.draw.context => {
            ui_write(unicorn, b, &1u64.to_le_bytes())?;
            json!({"call":"PF_GetDrawingReference","draw":1})
        }
        (7, 0 | 1) if a == 1 => {
            let id = if slot == 0 { 2u64 } else { 3 };
            ui_write(unicorn, b, &id.to_le_bytes())?;
            json!({"call":if slot==0 {"GetSupplier"} else {"GetSurface"},"object":id})
        }
        (8, 0 | 1 | 4 | 6) if a == 2 => {
            let kind = match slot {
                0 => "pen",
                1 => "brush",
                4 => "font",
                _ => "path",
            };
            let output = match slot {
                0 => d,
                1 | 4 => c,
                _ => b,
            };
            let color = if slot <= 1 {
                Some(ui_words(unicorn, b, 4)?)
            } else {
                None
            };
            let size = match slot {
                0 => Some(ui_float(unicorn, RegisterX86::XMM2)?),
                4 => Some(ui_float(unicorn, RegisterX86::XMM1)?),
                _ => None,
            };
            let id = ui_new_object(unicorn, kind, output)?;
            json!({"call":match slot {0=>"NewPen",1=>"NewBrush",4=>"NewDefaultFont",_=>"NewPath"},"object":id,"color_bits":color,"size_bits":size})
        }
        (8, 11 | 12) => {
            let objects = &mut unicorn.get_data_mut().ui_session.draw.objects;
            let (_, count) = objects
                .get_mut(&a)
                .ok_or("retain/release of stale Drawbot object")?;
            if slot == 11 {
                *count = count.checked_add(1).ok_or("Drawbot retain overflow")?;
            } else {
                *count -= 1;
            }
            let remaining = *count;
            if remaining == 0 {
                objects.remove(&a);
            }
            json!({"call":if slot==11 {"RetainObject"} else {"ReleaseObject"},"object":a,"references":remaining})
        }
        (9, 2) if a == 3 => {
            json!({"call":"PaintRect","color_bits":ui_words(unicorn,b,4)?,"rect_bits":ui_words(unicorn,c,4)?})
        }
        (9, 3 | 4) if a == 3 => {
            ui_object(unicorn, b, if slot == 3 { "brush" } else { "pen" })?;
            ui_object(unicorn, c, "path")?;
            json!({"call":if slot==3 {"FillPath"} else {"StrokePath"},"object":b,"path":c,"fill_type":if slot==3 {Some(d as u32)} else {None}})
        }
        (9, 9) if a == 3 => {
            ui_object(unicorn, b, "brush")?;
            ui_object(unicorn, c, "font")?;
            let mut text = Vec::new();
            for i in 0..4096 {
                let bytes = unicorn
                    .mem_read_as_vec(d + i * 2, 2)
                    .map_err(|e| e.to_string())?;
                let ch = u16::from_le_bytes(bytes.try_into().unwrap());
                if ch == 0 {
                    break;
                }
                text.push(ch);
            }
            if text.len() == 4096 {
                return Err("DrawString unterminated UTF16".into());
            }
            json!({"call":"DrawString","brush":b,"font":c,"utf16":text,
                "origin_bits":ui_words(unicorn,aegp_stack_arg(unicorn,0x28)?,2)?,
                "alignment":aegp_stack_arg(unicorn,0x30)? as u32,"truncation":aegp_stack_arg(unicorn,0x38)? as u32,
                "truncation_width_bits":aegp_stack_arg(unicorn,0x40)? as u32})
        }
        (9, 13) if a == 3 => json!({"call":"SetAntiAliasPolicy","policy":b as u32}),
        (10, 0 | 1 | 3 | 5) => {
            ui_object(unicorn, a, "path")?;
            match slot {
                0 | 1 => {
                    json!({"call":if slot==0 {"MoveTo"} else {"LineTo"},"path":a,"point_bits":[ui_float(unicorn,RegisterX86::XMM1)?,ui_float(unicorn,RegisterX86::XMM2)?]})
                }
                3 => json!({"call":"AddRect","path":a,"rect_bits":ui_words(unicorn,b,4)?}),
                _ => json!({"call":"Close","path":a}),
            }
        }
        (11, 1) => {
            let color = *unicorn
                .get_data()
                .ui_session
                .draw
                .theme
                .get(&(a as u16))
                .ok_or_else(|| format!("missing injected theme color {}", a as u16))?;
            let bytes: Vec<_> = color.into_iter().flat_map(u16::to_le_bytes).collect();
            ui_write(unicorn, b, &bytes)?;
            json!({"call":"PF_AppGetColor","role":a as u16,"rgb16":color})
        }
        (11, 7) => {
            let sample = ui_words(unicorn, b, 4)?;
            let picked = unicorn.get_data().ui_session.draw.picker;
            if let Some(color) = picked {
                ui_write(
                    unicorn,
                    d,
                    &color
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
            }
            // The callback wrapper writes RAX after this function returns.
            unicorn.get_data_mut().ui_session.return_code =
                if picked.is_some() { 0 } else { 0x205 };
            let mut title = Vec::new();
            if a != 0 {
                for i in 0..4096 {
                    let ch = unicorn
                        .mem_read_as_vec(a + i, 1)
                        .map_err(|e| e.to_string())?[0];
                    if ch == 0 {
                        break;
                    }
                    title.push(ch);
                }
                if title.len() == 4096 {
                    return Err("picker title exceeds bound".into());
                }
            }
            json!({"call":"PF_AppColorPickerDialog","title_bytes":title,"sample_argb_bits":sample,"transform":c as u8!=0,"result_argb_bits":picked})
        }
        (11, 9) if a == unicorn.get_data().ui_session.draw.context => {
            let rect = if b == 0 {
                None
            } else {
                Some(ui_words(unicorn, b, 4)?)
            };
            json!({"call":"PF_InvalidateRect","rect_bytes_u32":rect})
        }
        _ => return Err(format!("unsupported UI drawing suite {suite} slot {slot}")),
    };
    unicorn.get_data_mut().ui_session.calls.push(call);
    Ok(())
}
