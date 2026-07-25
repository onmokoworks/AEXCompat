use aex_abi::x86_64_windows as abi;
use std::collections::HashMap;
use thiserror::Error;
use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
use unicorn_engine::{RegisterX86, Unicorn};

use crate::pe::PeImage;

const PAGE_SIZE: u64 = 0x1000;
const STACK_BASE: u64 = 0x0000_0000_7000_0000;
const STACK_SIZE: u64 = 0x20_0000;
const STUB_BASE: u64 = 0x0000_0000_6000_0000;
const STUB_SIZE: u64 = 0x10_0000;
const STUB_STRIDE: u64 = 16;
const RETURN_ADDRESS: u64 = STUB_BASE + STUB_SIZE - PAGE_SIZE;
const HOST_ADD_PARAM: u64 = STUB_BASE + 0x80000;
const HOST_POISON: u64 = STUB_BASE + 0x80010;
const HOST_ANSI_STRCPY: u64 = STUB_BASE + 0x80020;
const HOST_COPY: u64 = STUB_BASE + 0x80030;
const HOST_NOOP: u64 = STUB_BASE + 0x80040;
const HOST_PRE_CHECKOUT_LAYER: u64 = STUB_BASE + 0x80050;
const HOST_CHECKOUT_LAYER_PIXELS: u64 = STUB_BASE + 0x80060;
const HOST_CHECKIN_LAYER_PIXELS: u64 = STUB_BASE + 0x80070;
const HOST_CHECKOUT_OUTPUT: u64 = STUB_BASE + 0x80080;
const HOST_ACQUIRE_SUITE: u64 = STUB_BASE + 0x80090;
const HOST_CHECKOUT_PARAM: u64 = STUB_BASE + 0x800a0;
const HOST_CHECKIN_PARAM: u64 = STUB_BASE + 0x800b0;
const HOST_NEW_HANDLE: u64 = STUB_BASE + 0x800c0;
const HOST_LOCK_HANDLE: u64 = STUB_BASE + 0x800d0;
const HOST_UNLOCK_HANDLE: u64 = STUB_BASE + 0x800e0;
const HOST_DISPOSE_HANDLE: u64 = STUB_BASE + 0x800f0;
const HOST_HANDLE_SIZE: u64 = STUB_BASE + 0x80100;
const HOST_RESIZE_HANDLE: u64 = STUB_BASE + 0x80110;
const HOST_HANDLE_SUITE: u64 = STUB_BASE + 0x81000;
const DATA_BASE: u64 = 0x0000_0000_4000_0000;
const DATA_SIZE: u64 = 0x1000_0000;
const HANDLE_DATA_BASE: u64 = DATA_BASE + 0x400_0000;
const HANDLE_DATA_END: u64 = DATA_BASE + DATA_SIZE;
// A nonzero Unicorn instruction limit enables instruction counting across the
// whole run, which is prohibitively expensive for image kernels. The wall-clock
// timeout and return-sentinel check still bound and validate guest execution.
const MAX_INSTRUCTIONS: usize = 0;
const TIMEOUT_MICROSECONDS: u64 = 600_000_000;

#[derive(Debug, Error)]
pub enum GuestError {
    #[error("unicorn error during {operation}: {detail}")]
    Unicorn {
        operation: &'static str,
        detail: String,
    },
    #[error("mapped PE image is not page aligned")]
    ImageAlignment,
    #[error("import stub capacity exceeded")]
    StubCapacity,
    #[error("IAT entry is outside the mapped image")]
    IatRange,
    #[error("guest data arena exhausted")]
    DataCapacity,
    #[error("guest callback failed: {0}")]
    Callback(String),
    #[error("DLL process attach returned FALSE")]
    DllProcessAttach,
}

fn uc<T>(
    operation: &'static str,
    result: Result<T, unicorn_engine::unicorn_const::uc_error>,
) -> Result<T, GuestError> {
    result.map_err(|error| GuestError::Unicorn {
        operation,
        detail: error.to_string(),
    })
}

#[derive(Clone, Debug)]
pub struct GuestParam {
    pub index: i32,
    pub param_type: i32,
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
struct GuestState {
    params: Vec<GuestParam>,
    callback_error: Option<String>,
    smart_input_world: u64,
    smart_output_world: u64,
    smart_width: u32,
    smart_height: u32,
    suite_requests: Vec<String>,
    pre_checkout_calls: u32,
    pre_checkout_requests: Vec<[i32; 4]>,
    checkout_pixels_calls: u32,
    checkout_output_calls: u32,
    parameter_definitions: Vec<u64>,
    next_handle_data: u64,
    handles: HashMap<u64, GuestHandle>,
    math_calls: Vec<String>,
    handle_allocations: Vec<u64>,
}

#[derive(Clone, Debug)]
struct GuestHandle {
    data: u64,
    size: u64,
    locks: u32,
}

pub struct GuestEngine<'a> {
    unicorn: Unicorn<'a, GuestState>,
    next_data: u64,
}

impl GuestEngine<'static> {
    pub fn load(image: &PeImage) -> Result<Self, GuestError> {
        let mut unicorn = uc(
            "create x86_64 engine",
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()),
        )?;
        unicorn.get_data_mut().next_handle_data = HANDLE_DATA_BASE;
        let image_size =
            u64::try_from(image.mapped_bytes().len()).map_err(|_| GuestError::ImageAlignment)?;
        if image.image_base() % PAGE_SIZE != 0 || image_size % PAGE_SIZE != 0 {
            return Err(GuestError::ImageAlignment);
        }
        uc(
            "map PE image",
            unicorn.mem_map(image.image_base(), image_size, Prot::ALL),
        )?;
        uc(
            "write PE image",
            unicorn.mem_write(image.image_base(), image.mapped_bytes()),
        )?;
        uc(
            "map import stubs",
            unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL),
        )?;
        uc(
            "map stack",
            unicorn.mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE),
        )?;
        // MSVC's x64 __chkstk reads the Windows TEB stack limit at GS:[0x10].
        // Unicorn starts with a zero GS base, so provide only the non-executable
        // first page needed by that helper.
        uc(
            "map minimal TEB page",
            unicorn.mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE),
        )?;
        uc(
            "write TEB stack limit",
            unicorn.mem_write(0x10, &STACK_BASE.to_le_bytes()),
        )?;
        uc(
            "map guest data",
            unicorn.mem_map(DATA_BASE, DATA_SIZE, Prot::READ | Prot::WRITE),
        )?;

        let mut stub_index = 0u64;
        for library in image.imports() {
            for symbol in &library.symbols {
                let stub = STUB_BASE
                    .checked_add(stub_index * STUB_STRIDE)
                    .ok_or(GuestError::StubCapacity)?;
                if stub + STUB_STRIDE > HOST_ADD_PARAM {
                    return Err(GuestError::StubCapacity);
                }
                // Temporary import behavior for the first controlled fixture:
                // return zero inside the guest. Typed import traps replace these
                // entries before OLM execution; no native host address is exposed.
                uc(
                    "write import stub",
                    unicorn.mem_write(stub, &[0x31, 0xc0, 0xc3]),
                )?;
                match symbol.name.as_str() {
                    "strncpy" => {
                        uc(
                            "install strncpy import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_strncpy(unicorn);
                            }),
                        )?;
                    }
                    "memset" => {
                        uc(
                            "install memset import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_memset(unicorn);
                            }),
                        )?;
                    }
                    "expf" => install_float_import(&mut unicorn, stub, "expf", f32::exp)?,
                    "floorf" => install_float_import(&mut unicorn, stub, "floorf", f32::floor)?,
                    "powf" => install_float_binary_import(&mut unicorn, stub, "powf", f32::powf)?,
                    "pow" => install_double_binary_import(&mut unicorn, stub, "pow", f64::powf)?,
                    _ => {}
                }
                let iat_rva = u64::try_from(symbol.iat_rva).map_err(|_| GuestError::IatRange)?;
                let iat = image
                    .image_base()
                    .checked_add(iat_rva)
                    .ok_or(GuestError::IatRange)?;
                if iat + 8 > image.image_base() + image_size {
                    return Err(GuestError::IatRange);
                }
                uc("patch IAT", unicorn.mem_write(iat, &stub.to_le_bytes()))?;
                stub_index += 1;
            }
        }
        uc(
            "write return sentinel",
            unicorn.mem_write(RETURN_ADDRESS, &[0xcc]),
        )?;
        uc(
            "write add_param callback",
            unicorn.mem_write(HOST_ADD_PARAM, &[0xc3]),
        )?;
        uc(
            "write poison callback",
            unicorn.mem_write(HOST_POISON, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3]),
        )?;
        uc(
            "write ANSI strcpy callback",
            unicorn.mem_write(HOST_ANSI_STRCPY, &[0xc3]),
        )?;
        uc("write copy callback", unicorn.mem_write(HOST_COPY, &[0xc3]))?;
        uc(
            "write no-op callback",
            unicorn.mem_write(HOST_NOOP, &[0x31, 0xc0, 0xc3]),
        )?;
        for (operation, address) in [
            ("write pre-checkout callback", HOST_PRE_CHECKOUT_LAYER),
            ("write checkout-pixels callback", HOST_CHECKOUT_LAYER_PIXELS),
            ("write checkin-pixels callback", HOST_CHECKIN_LAYER_PIXELS),
            ("write checkout-output callback", HOST_CHECKOUT_OUTPUT),
            ("write acquire-suite callback", HOST_ACQUIRE_SUITE),
            ("write checkout-param callback", HOST_CHECKOUT_PARAM),
            ("write checkin-param callback", HOST_CHECKIN_PARAM),
            ("write new-handle callback", HOST_NEW_HANDLE),
            ("write lock-handle callback", HOST_LOCK_HANDLE),
            ("write unlock-handle callback", HOST_UNLOCK_HANDLE),
            ("write dispose-handle callback", HOST_DISPOSE_HANDLE),
            ("write handle-size callback", HOST_HANDLE_SIZE),
            ("write resize-handle callback", HOST_RESIZE_HANDLE),
        ] {
            uc(operation, unicorn.mem_write(address, &[0xc3]))?;
        }
        uc(
            "install add_param callback",
            unicorn.add_code_hook(HOST_ADD_PARAM, HOST_ADD_PARAM, |unicorn, _, _| {
                capture_add_param(unicorn);
            }),
        )?;
        uc(
            "install ANSI strcpy callback",
            unicorn.add_code_hook(HOST_ANSI_STRCPY, HOST_ANSI_STRCPY, |unicorn, _, _| {
                emulate_strcpy(unicorn);
            }),
        )?;
        uc(
            "install copy callback",
            unicorn.add_code_hook(HOST_COPY, HOST_COPY, |unicorn, _, _| {
                emulate_copy(unicorn);
            }),
        )?;
        uc(
            "install pre-checkout callback",
            unicorn.add_code_hook(
                HOST_PRE_CHECKOUT_LAYER,
                HOST_PRE_CHECKOUT_LAYER,
                emulate_pre_checkout_layer,
            ),
        )?;
        uc(
            "install checkout-pixels callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_LAYER_PIXELS,
                HOST_CHECKOUT_LAYER_PIXELS,
                emulate_checkout_layer_pixels,
            ),
        )?;
        uc(
            "install checkin-pixels callback",
            unicorn.add_code_hook(
                HOST_CHECKIN_LAYER_PIXELS,
                HOST_CHECKIN_LAYER_PIXELS,
                |unicorn, _, _| {
                    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                },
            ),
        )?;
        uc(
            "install checkout-output callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_OUTPUT,
                HOST_CHECKOUT_OUTPUT,
                emulate_checkout_output,
            ),
        )?;
        uc(
            "install acquire-suite callback",
            unicorn.add_code_hook(
                HOST_ACQUIRE_SUITE,
                HOST_ACQUIRE_SUITE,
                emulate_acquire_suite,
            ),
        )?;
        uc(
            "install checkout-param callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_PARAM,
                HOST_CHECKOUT_PARAM,
                emulate_checkout_param,
            ),
        )?;
        uc(
            "install checkin-param callback",
            unicorn.add_code_hook(HOST_CHECKIN_PARAM, HOST_CHECKIN_PARAM, |unicorn, _, _| {
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            }),
        )?;
        for (operation, address, callback) in [
            (
                "install new-handle callback",
                HOST_NEW_HANDLE,
                emulate_new_handle as fn(&mut Unicorn<'_, GuestState>, u64, u32),
            ),
            (
                "install lock-handle callback",
                HOST_LOCK_HANDLE,
                emulate_lock_handle,
            ),
            (
                "install unlock-handle callback",
                HOST_UNLOCK_HANDLE,
                emulate_unlock_handle,
            ),
            (
                "install dispose-handle callback",
                HOST_DISPOSE_HANDLE,
                emulate_dispose_handle,
            ),
            (
                "install handle-size callback",
                HOST_HANDLE_SIZE,
                emulate_handle_size,
            ),
            (
                "install resize-handle callback",
                HOST_RESIZE_HANDLE,
                emulate_resize_handle,
            ),
        ] {
            uc(operation, unicorn.add_code_hook(address, address, callback))?;
        }
        let mut handle_suite = [0u8; 48];
        for (offset, address) in [
            HOST_NEW_HANDLE,
            HOST_LOCK_HANDLE,
            HOST_UNLOCK_HANDLE,
            HOST_DISPOSE_HANDLE,
            HOST_HANDLE_SIZE,
            HOST_RESIZE_HANDLE,
        ]
        .into_iter()
        .enumerate()
        {
            handle_suite[offset * 8..offset * 8 + 8].copy_from_slice(&address.to_le_bytes());
        }
        uc(
            "write PF Handle Suite",
            unicorn.mem_write(HOST_HANDLE_SUITE, &handle_suite),
        )?;
        let mut engine = Self {
            unicorn,
            next_data: DATA_BASE,
        };
        if let Some(entry) = image.dll_entry_address() {
            let attached = engine.call_win64(entry, [image.image_base(), 1, 0, 0, 0, 0])?;
            if attached == 0 {
                return Err(GuestError::DllProcessAttach);
            }
        }
        Ok(engine)
    }

    pub fn call_win64(&mut self, address: u64, args: [u64; 6]) -> Result<u64, GuestError> {
        self.call_win64_with_timeout(address, args, TIMEOUT_MICROSECONDS)
    }

    fn call_win64_with_timeout(
        &mut self,
        address: u64,
        args: [u64; 6],
        timeout_microseconds: u64,
    ) -> Result<u64, GuestError> {
        let stack_top = STACK_BASE + STACK_SIZE;
        // Win64 function entry observes RSP % 16 == 8. Reserve a return address,
        // 32-byte shadow space, two stack arguments, and bounded scratch.
        let rsp = (stack_top - 0x108) | 8;
        uc(
            "write return address",
            self.unicorn.mem_write(rsp, &RETURN_ADDRESS.to_le_bytes()),
        )?;
        uc(
            "write argument 5",
            self.unicorn.mem_write(rsp + 0x28, &args[4].to_le_bytes()),
        )?;
        uc(
            "write argument 6",
            self.unicorn.mem_write(rsp + 0x30, &args[5].to_le_bytes()),
        )?;
        for (register, value) in [
            (RegisterX86::RSP, rsp),
            (RegisterX86::RCX, args[0]),
            (RegisterX86::RDX, args[1]),
            (RegisterX86::R8, args[2]),
            (RegisterX86::R9, args[3]),
        ] {
            uc(
                "write argument register",
                self.unicorn.reg_write(register, value),
            )?;
        }
        if let Err(error) = self.unicorn.emu_start(
            address,
            RETURN_ADDRESS,
            timeout_microseconds,
            MAX_INSTRUCTIONS,
        ) {
            let rip = self.unicorn.reg_read(RegisterX86::RIP).unwrap_or(0);
            let rbx = self.unicorn.reg_read(RegisterX86::RBX).unwrap_or(0);
            let rcx = self.unicorn.reg_read(RegisterX86::RCX).unwrap_or(0);
            let rdx = self.unicorn.reg_read(RegisterX86::RDX).unwrap_or(0);
            let rbp = self.unicorn.reg_read(RegisterX86::RBP).unwrap_or(0);
            let r8 = self.unicorn.reg_read(RegisterX86::R8).unwrap_or(0);
            let suites = self.unicorn.get_data().suite_requests.join(", ");
            let math_calls = self.unicorn.get_data().math_calls.join(", ");
            let handle_allocations = &self.unicorn.get_data().handle_allocations;
            return Err(GuestError::Unicorn {
                operation: "execute guest function",
                detail: format!(
                    "{error} at RIP={rip:#x}, RBX={rbx:#x}, RBP={rbp:#x}, RCX={rcx:#x}, RDX={rdx:#x}, R8={r8:#x}, suite requests=[{suites}], handle allocations={handle_allocations:?}, math calls=[{math_calls}]"
                ),
            });
        }
        let rip = uc(
            "read instruction pointer",
            self.unicorn.reg_read(RegisterX86::RIP),
        )?;
        if rip != RETURN_ADDRESS {
            return Err(GuestError::Unicorn {
                operation: "execute guest function",
                detail: format!("execution stopped before the guest returned (RIP={rip:#x})"),
            });
        }
        if let Some(error) = self.unicorn.get_data_mut().callback_error.take() {
            return Err(GuestError::Callback(error));
        }
        uc("read return value", self.unicorn.reg_read(RegisterX86::RAX))
    }

    pub fn allocate(&mut self, size: usize, alignment: u64) -> Result<u64, GuestError> {
        let alignment = alignment.max(1).next_power_of_two();
        let start = self
            .next_data
            .checked_add(alignment - 1)
            .map(|value| value & !(alignment - 1))
            .ok_or(GuestError::DataCapacity)?;
        let end = start
            .checked_add(u64::try_from(size).map_err(|_| GuestError::DataCapacity)?)
            .ok_or(GuestError::DataCapacity)?;
        if end > DATA_BASE + DATA_SIZE {
            return Err(GuestError::DataCapacity);
        }
        self.next_data = end;
        Ok(start)
    }

    pub fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), GuestError> {
        uc("write guest data", self.unicorn.mem_write(address, bytes))
    }

    pub fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), GuestError> {
        uc("read guest data", self.unicorn.mem_read(address, bytes))
    }

    pub fn write_u64(&mut self, address: u64, value: u64) -> Result<(), GuestError> {
        self.write(address, &value.to_le_bytes())
    }

    pub fn add_param_callback_address(&self) -> u64 {
        HOST_ADD_PARAM
    }

    pub fn poison_callback_address(&self) -> u64 {
        HOST_POISON
    }

    pub fn ansi_strcpy_callback_address(&self) -> u64 {
        HOST_ANSI_STRCPY
    }

    pub fn copy_callback_address(&self) -> u64 {
        HOST_COPY
    }

    pub fn noop_callback_address(&self) -> u64 {
        HOST_NOOP
    }

    pub fn acquire_suite_callback_address(&self) -> u64 {
        HOST_ACQUIRE_SUITE
    }

    pub fn checkout_param_callback_address(&self) -> u64 {
        HOST_CHECKOUT_PARAM
    }

    pub fn checkin_param_callback_address(&self) -> u64 {
        HOST_CHECKIN_PARAM
    }

    pub fn configure_parameter_definitions(&mut self, definitions: Vec<u64>) {
        self.unicorn.get_data_mut().parameter_definitions = definitions;
    }

    pub fn suite_requests(&self) -> &[String] {
        &self.unicorn.get_data().suite_requests
    }

    pub fn smart_callback_counts(&self) -> (u32, u32, u32) {
        let state = self.unicorn.get_data();
        (
            state.pre_checkout_calls,
            state.checkout_pixels_calls,
            state.checkout_output_calls,
        )
    }

    pub fn pre_checkout_requests(&self) -> &[[i32; 4]] {
        &self.unicorn.get_data().pre_checkout_requests
    }

    pub fn handle_allocations(&self) -> &[u64] {
        &self.unicorn.get_data().handle_allocations
    }

    pub fn pre_checkout_layer_callback_address(&self) -> u64 {
        HOST_PRE_CHECKOUT_LAYER
    }

    pub fn checkout_layer_pixels_callback_address(&self) -> u64 {
        HOST_CHECKOUT_LAYER_PIXELS
    }

    pub fn checkin_layer_pixels_callback_address(&self) -> u64 {
        HOST_CHECKIN_LAYER_PIXELS
    }

    pub fn checkout_output_callback_address(&self) -> u64 {
        HOST_CHECKOUT_OUTPUT
    }

    pub fn new_handle_callback_address(&self) -> u64 {
        HOST_NEW_HANDLE
    }

    pub fn lock_handle_callback_address(&self) -> u64 {
        HOST_LOCK_HANDLE
    }

    pub fn unlock_handle_callback_address(&self) -> u64 {
        HOST_UNLOCK_HANDLE
    }

    pub fn dispose_handle_callback_address(&self) -> u64 {
        HOST_DISPOSE_HANDLE
    }

    pub fn handle_size_callback_address(&self) -> u64 {
        HOST_HANDLE_SIZE
    }

    pub fn resize_handle_callback_address(&self) -> u64 {
        HOST_RESIZE_HANDLE
    }

    pub fn configure_smart_render(
        &mut self,
        input_world: u64,
        output_world: u64,
        width: u32,
        height: u32,
    ) {
        let state = self.unicorn.get_data_mut();
        state.pre_checkout_requests.clear();
        state.smart_input_world = input_world;
        state.smart_output_world = output_world;
        state.smart_width = width;
        state.smart_height = height;
    }

    pub fn parameters(&self) -> &[GuestParam] {
        &self.unicorn.get_data().params
    }
}

fn capture_add_param(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("read add_param index: {error}"))? as i32;
        let pointer = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("read add_param pointer: {error}"))?;
        let mut bytes = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        unicorn
            .mem_read(pointer, &mut bytes)
            .map_err(|error| format!("read PF_ParamDef at {pointer:#x}: {error}"))?;
        let param_type = i32::from_le_bytes(
            bytes[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
                .try_into()
                .expect("generated PF_ParamDef field is four bytes"),
        );
        let name_bytes =
            &bytes[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + abi::PARAM_NAME_SIZE];
        let name_end = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_bytes.len());
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
        Ok(GuestParam {
            index,
            param_type,
            name,
            bytes,
        })
    })();
    match result {
        Ok(param) => {
            unicorn.get_data_mut().params.push(param);
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn install_float_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f32) -> f32,
) -> Result<(), GuestError> {
    uc(
        "install unary float import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let Ok(bits) = unicorn.reg_read(RegisterX86::XMM0) {
                let value = f32::from_bits(bits as u32);
                let output = operation(value);
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn
                        .get_data_mut()
                        .math_calls
                        .push(format!("{name}({value})={output}"));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, output.to_bits() as u64);
            }
        }),
    )
    .map(|_| ())
}

fn install_float_binary_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f32, f32) -> f32,
) -> Result<(), GuestError> {
    uc(
        "install binary float import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let (Ok(left), Ok(right)) = (
                unicorn.reg_read(RegisterX86::XMM0),
                unicorn.reg_read(RegisterX86::XMM1),
            ) {
                let value = operation(f32::from_bits(left as u32), f32::from_bits(right as u32));
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn.get_data_mut().math_calls.push(format!(
                        "{name}({},{})={value}",
                        f32::from_bits(left as u32),
                        f32::from_bits(right as u32)
                    ));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, value.to_bits() as u64);
            }
        }),
    )
    .map(|_| ())
}

fn install_double_binary_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f64, f64) -> f64,
) -> Result<(), GuestError> {
    uc(
        "install binary double import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let (Ok(left), Ok(right)) = (
                unicorn.reg_read(RegisterX86::XMM0),
                unicorn.reg_read(RegisterX86::XMM1),
            ) {
                let value = operation(f64::from_bits(left), f64::from_bits(right));
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn.get_data_mut().math_calls.push(format!(
                        "{name}({},{})={value}",
                        f64::from_bits(left),
                        f64::from_bits(right)
                    ));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, value.to_bits());
            }
        }),
    )
    .map(|_| ())
}

fn emulate_memset(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("memset destination: {error}"))?;
        let value = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("memset value: {error}"))? as u8;
        let length = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("memset length: {error}"))?;
        if length > DATA_SIZE {
            return Err(format!("memset length exceeds guest data bound: {length}"));
        }
        let bytes = vec![value; length as usize];
        unicorn
            .mem_write(destination, &bytes)
            .map_err(|error| format!("memset write: {error}"))?;
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_strncpy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("strncpy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("strncpy source: {error}"))?;
        let count = usize::try_from(
            unicorn
                .reg_read(RegisterX86::R8)
                .map_err(|error| format!("strncpy count: {error}"))?,
        )
        .map_err(|_| "strncpy count does not fit usize".to_string())?;
        if count > 4096 {
            return Err(format!("strncpy count {count} exceeds 4096"));
        }
        let mut output = vec![0u8; count];
        let mut terminated = false;
        for (index, byte) in output.iter_mut().enumerate() {
            if terminated {
                *byte = 0;
                continue;
            }
            let mut source_byte = [0u8; 1];
            unicorn
                .mem_read(source + index as u64, &mut source_byte)
                .map_err(|error| format!("strncpy source read: {error}"))?;
            *byte = source_byte[0];
            terminated = source_byte[0] == 0;
        }
        unicorn
            .mem_write(destination, &output)
            .map_err(|error| format!("strncpy destination write: {error}"))?;
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_strcpy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("strcpy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("strcpy source: {error}"))?;
        for index in 0..4096u64 {
            let mut byte = [0u8; 1];
            unicorn
                .mem_read(source + index, &mut byte)
                .map_err(|error| format!("strcpy source read: {error}"))?;
            unicorn
                .mem_write(destination + index, &byte)
                .map_err(|error| format!("strcpy destination write: {error}"))?;
            if byte[0] == 0 {
                return Ok(destination);
            }
        }
        Err("strcpy source exceeds 4096 bytes".to_string())
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_copy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("copy source world: {error}"))?;
        let destination = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("copy destination world: {error}"))?;
        let read_u64 = |unicorn: &Unicorn<'_, GuestState>, address| {
            let mut bytes = [0u8; 8];
            unicorn
                .mem_read(address, &mut bytes)
                .map_err(|error| format!("copy world pointer read: {error}"))?;
            Ok::<u64, String>(u64::from_le_bytes(bytes))
        };
        let read_i32 = |unicorn: &Unicorn<'_, GuestState>, address| {
            let mut bytes = [0u8; 4];
            unicorn
                .mem_read(address, &mut bytes)
                .map_err(|error| format!("copy world field read: {error}"))?;
            Ok::<i32, String>(i32::from_le_bytes(bytes))
        };
        let source_data = read_u64(unicorn, source + abi::LAYER_DATA_OFFSET as u64)?;
        let destination_data = read_u64(unicorn, destination + abi::LAYER_DATA_OFFSET as u64)?;
        let source_rowbytes =
            read_i32(unicorn, source + abi::LAYER_ROWBYTES_OFFSET as u64)?.max(0) as usize;
        let destination_rowbytes =
            read_i32(unicorn, destination + abi::LAYER_ROWBYTES_OFFSET as u64)?.max(0) as usize;
        let height = read_i32(unicorn, source + abi::LAYER_HEIGHT_OFFSET as u64)?
            .min(read_i32(
                unicorn,
                destination + abi::LAYER_HEIGHT_OFFSET as u64,
            )?)
            .max(0) as usize;
        let row_size = source_rowbytes.min(destination_rowbytes);
        for row in 0..height {
            let mut bytes = vec![0u8; row_size];
            unicorn
                .mem_read(source_data + (row * source_rowbytes) as u64, &mut bytes)
                .map_err(|error| format!("copy source pixels: {error}"))?;
            unicorn
                .mem_write(
                    destination_data + (row * destination_rowbytes) as u64,
                    &bytes,
                )
                .map_err(|error| format!("copy destination pixels: {error}"))?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_pre_checkout_layer(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().pre_checkout_calls += 1;
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("pre-checkout index: {error}"))? as i32;
        let checkout_id = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("pre-checkout id: {error}"))? as i32;
        if index != 0 || checkout_id != 0 {
            return Err(format!(
                "unsupported smart checkout index={index} id={checkout_id}"
            ));
        }
        let request_pointer = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("pre-checkout request: {error}"))?;
        if request_pointer == 0 {
            return Err("pre-checkout request is null".to_string());
        }
        let mut request_rect_bytes = [0u8; 16];
        unicorn
            .mem_read(request_pointer, &mut request_rect_bytes)
            .map_err(|error| format!("pre-checkout request rect: {error}"))?;
        let mut request_rect = [0i32; 4];
        for (index, value) in request_rect.iter_mut().enumerate() {
            let offset = index * 4;
            *value = i32::from_le_bytes(
                request_rect_bytes[offset..offset + 4]
                    .try_into()
                    .expect("render request rectangle element is four bytes"),
            );
        }
        unicorn
            .get_data_mut()
            .pre_checkout_requests
            .push(request_rect);
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("pre-checkout stack: {error}"))?;
        let mut result_pointer = [0u8; 8];
        unicorn
            .mem_read(rsp + 0x40, &mut result_pointer)
            .map_err(|error| format!("pre-checkout result pointer: {error}"))?;
        let result_pointer = u64::from_le_bytes(result_pointer);
        if result_pointer == 0 {
            return Err("pre-checkout result is null".to_string());
        }
        let state = unicorn.get_data();
        let width = state.smart_width as i32;
        let height = state.smart_height as i32;
        let mut bytes = [0u8; 76];
        for (offset, value) in [
            (0, 0),
            (4, 0),
            (8, width),
            (12, height),
            (16, 0),
            (20, 0),
            (24, width),
            (28, height),
            (32, 1),
            (36, 1),
            (44, width),
            (48, height),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        unicorn
            .mem_write(result_pointer, &bytes)
            .map_err(|error| format!("pre-checkout result write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_checkout_layer_pixels(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().checkout_pixels_calls += 1;
    let result = (|| {
        let checkout_id = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-pixels id: {error}"))?
            as i32;
        let output = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("checkout-pixels output: {error}"))?;
        let input_world = unicorn.get_data().smart_input_world;
        if checkout_id != 0 || output == 0 || input_world == 0 {
            return Err(format!(
                "invalid checkout-pixels id={checkout_id} output={output:#x}"
            ));
        }
        unicorn
            .mem_write(output, &input_world.to_le_bytes())
            .map_err(|error| format!("checkout-pixels world write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_checkout_output(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().checkout_output_calls += 1;
    let result = (|| {
        let output = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-output pointer: {error}"))?;
        let output_world = unicorn.get_data().smart_output_world;
        if output == 0 || output_world == 0 {
            return Err(format!("invalid checkout-output pointer={output:#x}"));
        }
        unicorn
            .mem_write(output, &output_world.to_le_bytes())
            .map_err(|error| format!("checkout-output world write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_acquire_suite(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let name_pointer = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let version = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let mut bytes = Vec::new();
    if name_pointer != 0 {
        for offset in 0..256u64 {
            let mut byte = [0u8; 1];
            if unicorn.mem_read(name_pointer + offset, &mut byte).is_err() || byte[0] == 0 {
                break;
            }
            bytes.push(byte[0]);
        }
    }
    let name = String::from_utf8_lossy(&bytes);
    unicorn
        .get_data_mut()
        .suite_requests
        .push(format!("{name} v{version}"));
    let output = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    if name == "PF Handle Suite" && version == 2 && output != 0 {
        if unicorn
            .mem_write(output, &HOST_HANDLE_SUITE.to_le_bytes())
            .is_ok()
        {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            return;
        }
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
}

fn emulate_checkout_param(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-param index: {error}"))?
            as usize;
        let source = index
            .checked_sub(1)
            .and_then(|offset| unicorn.get_data().parameter_definitions.get(offset))
            .copied()
            .ok_or_else(|| format!("checkout-param index is outside definitions: {index}"))?;
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("checkout-param stack: {error}"))?;
        let mut destination = [0u8; 8];
        unicorn
            .mem_read(rsp + 0x30, &mut destination)
            .map_err(|error| format!("checkout-param destination pointer: {error}"))?;
        let destination = u64::from_le_bytes(destination);
        if destination == 0 {
            return Err("checkout-param destination is null".to_string());
        }
        let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        unicorn
            .mem_read(source, &mut definition)
            .map_err(|error| format!("checkout-param definition read: {error}"))?;
        unicorn
            .mem_write(destination, &definition)
            .map_err(|error| format!("checkout-param definition write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_new_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let size = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX);
    unicorn.get_data_mut().handle_allocations.push(size);
    let allocation = (|| {
        if size > 128 * 1024 * 1024 {
            return Err(format!("handle allocation exceeds 128 MiB: {size}"));
        }
        let state = unicorn.get_data_mut();
        let handle = (state.next_handle_data + 7) & !7;
        let data = (handle + 8 + 15) & !15;
        let end = data
            .checked_add(size.max(1))
            .ok_or_else(|| "handle allocation overflow".to_string())?;
        if end > HANDLE_DATA_END {
            return Err("handle arena exhausted".to_string());
        }
        state.next_handle_data = end;
        state.handles.insert(
            handle,
            GuestHandle {
                data,
                size,
                locks: 0,
            },
        );
        Ok((handle, data))
    })();
    match allocation {
        Ok((handle, data)) => {
            let _ = unicorn.mem_write(handle, &data.to_le_bytes());
            if size != 0 {
                let _ = unicorn.mem_write(data, &vec![0u8; size as usize]);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, handle);
        }
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
    }
}

fn emulate_lock_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let data = unicorn
        .get_data_mut()
        .handles
        .get_mut(&handle)
        .map(|record| {
            record.locks = record.locks.saturating_add(1);
            record.data
        });
    let _ = unicorn.reg_write(RegisterX86::RAX, data.unwrap_or_default());
}

fn emulate_unlock_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    if let Some(record) = unicorn.get_data_mut().handles.get_mut(&handle) {
        record.locks = record.locks.saturating_sub(1);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_dispose_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    unicorn.get_data_mut().handles.remove(&handle);
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_handle_size(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let size = unicorn
        .get_data()
        .handles
        .get(&handle)
        .map(|record| record.size)
        .unwrap_or_default();
    let _ = unicorn.reg_write(RegisterX86::RAX, size);
}

fn emulate_resize_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let size = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("resize-handle size: {error}"))?;
        let handle_pointer = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("resize-handle pointer: {error}"))?;
        if size > 128 * 1024 * 1024 || handle_pointer == 0 {
            return Err("invalid resize-handle request".to_string());
        }
        let mut handle_bytes = [0u8; 8];
        unicorn
            .mem_read(handle_pointer, &mut handle_bytes)
            .map_err(|error| format!("resize-handle read: {error}"))?;
        let handle = u64::from_le_bytes(handle_bytes);
        let old = unicorn
            .get_data()
            .handles
            .get(&handle)
            .cloned()
            .ok_or_else(|| "resize-handle unknown handle".to_string())?;
        if old.locks != 0 {
            return Err("resize-handle locked handle".to_string());
        }
        let data = {
            let state = unicorn.get_data_mut();
            let data = (state.next_handle_data + 15) & !15;
            let end = data
                .checked_add(size.max(1))
                .ok_or_else(|| "resize-handle overflow".to_string())?;
            if end > HANDLE_DATA_END {
                return Err("handle arena exhausted".to_string());
            }
            state.next_handle_data = end;
            data
        };
        let mut bytes = vec![0u8; size as usize];
        let copied = old.size.min(size) as usize;
        if copied != 0 {
            unicorn
                .mem_read(old.data, &mut bytes[..copied])
                .map_err(|error| format!("resize-handle old data: {error}"))?;
        }
        if size != 0 {
            unicorn
                .mem_write(data, &bytes)
                .map_err(|error| format!("resize-handle new data: {error}"))?;
        }
        unicorn
            .mem_write(handle, &data.to_le_bytes())
            .map_err(|error| format!("resize-handle record: {error}"))?;
        if let Some(record) = unicorn.get_data_mut().handles.get_mut(&handle) {
            record.data = data;
            record.size = size;
        }
        Ok(())
    })();
    let _ = unicorn.reg_write(RegisterX86::RAX, if result.is_ok() { 0 } else { 4 });
}

fn finish_callback(unicorn: &mut Unicorn<'_, GuestState>, result: Result<(), String>) {
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine(code: &[u8]) -> GuestEngine<'static> {
        const CODE: u64 = 0x1000_0000;
        let mut unicorn =
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
        unicorn.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
        unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL).unwrap();
        unicorn
            .mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        unicorn.mem_write(CODE, code).unwrap();
        unicorn.mem_write(RETURN_ADDRESS, &[0xcc]).unwrap();
        GuestEngine {
            unicorn,
            next_data: DATA_BASE,
        }
    }

    #[test]
    fn win64_call_places_register_arguments_and_returns_rax() {
        const CODE: u64 = 0x1000_0000;
        // mov rax, rcx; add rax, rdx; ret
        let mut engine = test_engine(&[0x48, 0x89, 0xc8, 0x48, 0x01, 0xd0, 0xc3]);
        assert_eq!(engine.call_win64(CODE, [40, 2, 0, 0, 0, 0]).unwrap(), 42);
    }

    #[test]
    fn win64_call_rejects_execution_that_does_not_reach_return_sentinel() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[0xf4]); // hlt
        let error = engine.call_win64(CODE, [0; 6]).unwrap_err().to_string();
        assert!(error.contains("before the guest returned"), "{error}");
    }

    #[test]
    fn win64_call_timeout_still_fails_closed() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[0xeb, 0xfe]); // jmp $
        let error = engine
            .call_win64_with_timeout(CODE, [0; 6], 1_000)
            .unwrap_err()
            .to_string();
        assert!(error.contains("before the guest returned"), "{error}");
    }
}
