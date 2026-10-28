//! Opt-in instruction-site observations. The portable core never dereferences
//! host pointers; the Windows adapter supplies exact, bounded remote reads.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub module: String,
    pub watches: Vec<Watch>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    pub id: String,
    pub instruction_rva: u64,
    pub register: Register,
    #[serde(default)]
    pub dereference_offsets: Vec<u64>,
    pub size: usize,
    pub occurrence: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Register {
    Rax,
    Rbx,
    Rcx,
    Rdx,
    Rsi,
    Rdi,
    Rbp,
    Rsp,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub status: &'static str,
    pub address: Option<u64>,
    pub size: usize,
    pub hex: Option<String>,
    pub pointer_chain: Vec<u64>,
}

#[derive(Debug, Serialize)]
pub struct Witness {
    pub watch_id: String,
    pub instruction_rva: u64,
    pub occurrence: u64,
    pub sequence: u64,
    pub thread_id: u32,
    pub phase: &'static str,
    pub before: Snapshot,
}

pub trait Memory {
    /// Must either fill the entire buffer or return false. Partial bytes are
    /// never published as captured data.
    fn read_exact(&mut self, address: u64, bytes: &mut [u8]) -> bool;
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.module.is_empty()
            || self.module.len() > 255
            || self
                .module
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        {
            return Err("module must be a basename".into());
        }
        if self.watches.is_empty() || self.watches.len() > 4 {
            return Err("hardware transport requires 1..4 watches".into());
        }
        let mut ids = BTreeSet::new();
        let mut sites = BTreeSet::new();
        let mut total = 0usize;
        for w in &self.watches {
            if w.id.is_empty()
                || w.id.len() > 80
                || w.id.chars().any(char::is_control)
                || !ids.insert(&w.id)
            {
                return Err("watch ids must be nonempty and unique (max 80 chars)".into());
            }
            if !sites.insert(w.instruction_rva) {
                return Err("each hardware watch needs a distinct instruction RVA".into());
            }
            if w.occurrence == 0
                || w.size == 0
                || w.size > MAX_BYTES
                || w.dereference_offsets.len() > 8
                || w.dereference_offsets.iter().any(|o| *o > 4096)
            {
                return Err("invalid occurrence, size or bounded pointer chain".into());
            }
            total += w.size;
        }
        if total > MAX_TOTAL_BYTES {
            return Err("declared capture total exceeds 32 MiB".into());
        }
        Ok(())
    }
}

pub struct Observer {
    pub config: Config,
    pub hits: Vec<u64>,
    pub witnesses: Vec<Witness>,
    sequence: u64,
}

impl Observer {
    pub fn new(config: Config) -> Result<Self, String> {
        config.validate()?;
        Ok(Self {
            hits: vec![0; config.watches.len()],
            config,
            witnesses: Vec::new(),
            sequence: 0,
        })
    }

    pub fn addresses(&self, base: u64, size: u64) -> Result<Vec<u64>, String> {
        self.config
            .watches
            .iter()
            .map(|w| {
                if w.instruction_rva >= size {
                    return Err("watch RVA outside loaded module".into());
                }
                base.checked_add(w.instruction_rva)
                    .ok_or_else(|| "module address overflow".into())
            })
            .collect()
    }

    /// Called once per hardware execution breakpoint, before the instruction.
    /// Occurrences are global across threads in debugger event order, per RVA.
    pub fn hit(
        &mut self,
        rva: u64,
        thread_id: u32,
        registers: &BTreeMap<Register, u64>,
        memory: &mut impl Memory,
    ) -> Result<(), String> {
        let Some(index) = self
            .config
            .watches
            .iter()
            .position(|w| w.instruction_rva == rva)
        else {
            return Ok(());
        };
        self.hits[index] = self.hits[index]
            .checked_add(1)
            .ok_or("occurrence overflow")?;
        self.sequence = self.sequence.checked_add(1).ok_or("sequence overflow")?;
        let w = &self.config.watches[index];
        if self.hits[index] != w.occurrence {
            return Ok(());
        }
        let before = capture(w, registers.get(&w.register).copied(), memory);
        self.witnesses.push(Witness {
            watch_id: w.id.clone(),
            instruction_rva: rva,
            occurrence: self.hits[index],
            sequence: self.sequence,
            thread_id,
            phase: "instruction_before",
            before,
        });
        Ok(())
    }

    pub fn complete(&self) -> bool {
        self.witnesses.len() == self.config.watches.len()
    }
    pub fn captured(&self) -> bool {
        self.complete() && self.witnesses.iter().all(|w| w.before.status == "captured")
    }
}

fn capture(w: &Watch, register: Option<u64>, memory: &mut impl Memory) -> Snapshot {
    let mut snap = Snapshot {
        status: "missing_register",
        address: register,
        size: w.size,
        hex: None,
        pointer_chain: Vec::new(),
    };
    let Some(mut address) = register else {
        return snap;
    };
    for offset in &w.dereference_offsets {
        let Some(at) = address
            .checked_add(*offset)
            .filter(|a| a.checked_add(7).is_some())
        else {
            snap.status = "address_overflow";
            return snap;
        };
        let mut bytes = [0; 8];
        if !memory.read_exact(at, &mut bytes) {
            snap.status = "pointer_unreadable";
            return snap;
        }
        address = u64::from_le_bytes(bytes);
        snap.pointer_chain.push(address);
        snap.address = Some(address);
    }
    if address.checked_add(w.size as u64 - 1).is_none() {
        snap.status = "address_overflow";
        return snap;
    }
    let mut bytes = vec![0; w.size];
    if !memory.read_exact(address, &mut bytes) {
        snap.status = "unreadable";
        return snap;
    }
    use std::fmt::Write;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(hex, "{b:02x}").unwrap();
    }
    snap.status = "captured";
    snap.hex = Some(hex);
    snap
}

#[cfg(test)]
mod tests;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod windows;
