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

pub struct GuestEngine<'a> {
    unicorn: Unicorn<'a, ()>,
}

impl GuestEngine<'static> {
    pub fn load(image: &PeImage) -> Result<Self, GuestError> {
        let mut unicorn = uc(
            "create x86_64 engine",
            Unicorn::new(Arch::X86, Mode::MODE_64),
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
            "map import stubs",
            unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL),
        )?;
        uc(
            "map stack",
            unicorn.mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE),
        )?;

        let mut stub_index = 0u64;
        for library in image.imports() {
            for symbol in &library.symbols {
                let stub = STUB_BASE
                    .checked_add(stub_index * STUB_STRIDE)
                    .ok_or(GuestError::StubCapacity)?;
                if stub + STUB_STRIDE > RETURN_ADDRESS {
                    return Err(GuestError::StubCapacity);
                }
                // Conservative placeholder: every unimplemented import returns zero.
                // Individual imports are replaced with typed traps as they become
                // necessary; unknown imports never call native host addresses.
                uc(
                    "write import stub",
                    unicorn.mem_write(stub, &[0x31, 0xc0, 0xc3]),
                )?;
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
        Ok(Self { unicorn })
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
        uc(
            "execute guest function",
            self.unicorn.emu_start(
                address,
                RETURN_ADDRESS,
                TIMEOUT_MICROSECONDS,
                MAX_INSTRUCTIONS,
            ),
        )?;
        uc("read return value", self.unicorn.reg_read(RegisterX86::RAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win64_call_places_register_arguments_and_returns_rax() {
        const CODE: u64 = 0x1000_0000;
        let mut unicorn = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
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
        let mut engine = GuestEngine { unicorn };
        assert_eq!(engine.call_win64(CODE, [40, 2, 0, 0, 0, 0]).unwrap(), 42);
    }
}
