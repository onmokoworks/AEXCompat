use aex_abi::x86_64_windows as abi;
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
const DATA_BASE: u64 = 0x0000_0000_4000_0000;
const DATA_SIZE: u64 = 0x10_0000;
const MAX_INSTRUCTIONS: usize = 20_000_000;
const TIMEOUT_MICROSECONDS: u64 = 5_000_000;

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
    last_pc: u64,
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
            "install image execution trace",
            unicorn.add_code_hook(
                image.image_base(),
                image.image_base() + image_size - 1,
                |unicorn, address, _| {
                    unicorn.get_data_mut().last_pc = address;
                },
            ),
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
                if symbol.name == "strncpy" {
                    uc(
                        "install strncpy import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_strncpy(unicorn);
                        }),
                    )?;
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
        Ok(Self {
            unicorn,
            next_data: DATA_BASE,
        })
    }

    pub fn call_win64(&mut self, address: u64, args: [u64; 6]) -> Result<u64, GuestError> {
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
            TIMEOUT_MICROSECONDS,
            MAX_INSTRUCTIONS,
        ) {
            let rip = self.unicorn.reg_read(RegisterX86::RIP).unwrap_or(0);
            let last_pc = self.unicorn.get_data().last_pc;
            let rbx = self.unicorn.reg_read(RegisterX86::RBX).unwrap_or(0);
            return Err(GuestError::Unicorn {
                operation: "execute guest function",
                detail: format!(
                    "{error} at RIP={rip:#x}, last guest PC={last_pc:#x}, RBX={rbx:#x}"
                ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win64_call_places_register_arguments_and_returns_rax() {
        const CODE: u64 = 0x1000_0000;
        let mut unicorn =
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
        unicorn.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
        unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL).unwrap();
        unicorn
            .mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        // mov rax, rcx; add rax, rdx; ret
        unicorn
            .mem_write(CODE, &[0x48, 0x89, 0xc8, 0x48, 0x01, 0xd0, 0xc3])
            .unwrap();
        unicorn.mem_write(RETURN_ADDRESS, &[0xcc]).unwrap();
        let mut engine = GuestEngine {
            unicorn,
            next_data: DATA_BASE,
        };
        assert_eq!(engine.call_win64(CODE, [40, 2, 0, 0, 0, 0]).unwrap(), 42);
    }
}
