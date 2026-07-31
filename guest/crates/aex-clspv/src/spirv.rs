use std::collections::{BTreeMap, BTreeSet};
use std::{error::Error, fmt};

const SPIRV_MAGIC: u32 = 0x0723_0203;
const SPIRV_VERSION_1_0: u32 = 0x0001_0000;
const HEADER_WORDS: usize = 5;
const MAX_SPIRV_BYTES: usize = 16 * 1024 * 1024;
const MAX_ID_BOUND: u32 = 1_000_000;
const MAX_INSTRUCTIONS: usize = 1_000_000;
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_KERNELS: usize = 256;
const MAX_ARGUMENTS_PER_KERNEL: u32 = 1_024;
const MAX_DESCRIPTOR_SET: u32 = 31;
const MAX_BINDING: u32 = 1_023;
const MAX_POD_BYTES: u32 = 64 * 1024;

const OP_STRING: u16 = 7;
const OP_EXTENSION: u16 = 10;
const OP_EXT_INST_IMPORT: u16 = 11;
const OP_EXT_INST: u16 = 12;
const OP_MEMORY_MODEL: u16 = 14;
const OP_ENTRY_POINT: u16 = 15;
const OP_EXECUTION_MODE: u16 = 16;
const OP_TYPE_VOID: u16 = 19;
const OP_TYPE_INT: u16 = 21;
const OP_CONSTANT: u16 = 43;
const OP_SPEC_CONSTANT: u16 = 50;
const OP_FUNCTION: u16 = 54;
const OP_VARIABLE: u16 = 59;
const OP_LOAD: u16 = 61;
const OP_STORE: u16 = 62;
const OP_DECORATE: u16 = 71;

const EXECUTION_MODEL_GL_COMPUTE: u32 = 5;
const EXECUTION_MODE_LOCAL_SIZE: u32 = 17;
const ADDRESSING_MODEL_LOGICAL: u32 = 0;
const MEMORY_MODEL_GLSL450: u32 = 1;
const CAPABILITY_SHADER: u32 = 1;
const STORAGE_CLASS_UNIFORM: u32 = 2;
const STORAGE_CLASS_STORAGE_BUFFER: u32 = 12;
const DECORATION_SPEC_ID: u32 = 1;
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;
const MEMORY_ACCESS_ALIGNED: u32 = 0x2;

const EXT_STORAGE_BUFFER: &str = "SPV_KHR_storage_buffer_storage_class";
const EXT_NONSEMANTIC: &str = "SPV_KHR_non_semantic_info";
const EXT_GLSL_STD_450: &str = "GLSL.std.450";
const CLSPV_REFLECTION_IMPORT: &str = "NonSemantic.ClspvReflection.5";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelArgumentKind {
    StorageBuffer,
    PodUniform { offset: u32, size: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelArgument {
    pub ordinal: u32,
    pub descriptor_set: u32,
    pub binding: u32,
    pub name: Option<String>,
    pub kind: KernelArgumentKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelReflection {
    pub name: String,
    pub function_id: u32,
    pub declared_argument_count: u32,
    pub arguments: Vec<KernelArgument>,
    pub required_workgroup_size: Option<[u32; 3]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkgroupSpecIds {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpirvReflection {
    kernels: Vec<KernelReflection>,
    workgroup_spec_ids: Option<WorkgroupSpecIds>,
    work_dim_spec_id: Option<u32>,
}

impl SpirvReflection {
    pub fn kernels(&self) -> &[KernelReflection] {
        &self.kernels
    }

    pub fn kernel(&self, name: &str) -> Option<&KernelReflection> {
        self.kernels.iter().find(|kernel| kernel.name == name)
    }

    pub const fn workgroup_spec_ids(&self) -> Option<WorkgroupSpecIds> {
        self.workgroup_spec_ids
    }

    pub const fn work_dim_spec_id(&self) -> Option<u32> {
        self.work_dim_spec_id
    }
}

#[derive(Clone)]
pub struct SpirvModule {
    raw_words: Vec<u32>,
    stripped_words: Vec<u32>,
    reflection: SpirvReflection,
}

impl SpirvModule {
    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, ReflectionError> {
        if bytes.len() > MAX_SPIRV_BYTES {
            return Err(ReflectionError::LimitExceeded {
                item: "SPIR-V bytes",
                actual: bytes.len() as u64,
                maximum: MAX_SPIRV_BYTES as u64,
            });
        }
        if bytes.len() % 4 != 0 {
            return Err(ReflectionError::Malformed(
                "SPIR-V byte length is not a multiple of four".into(),
            ));
        }
        let words = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte chunk")))
            .collect::<Vec<_>>();
        Self::parse_words(words)
    }

    pub fn raw_words(&self) -> &[u32] {
        &self.raw_words
    }

    pub fn stripped_words(&self) -> &[u32] {
        &self.stripped_words
    }

    pub const fn reflection(&self) -> &SpirvReflection {
        &self.reflection
    }

    fn parse_words(words: Vec<u32>) -> Result<Self, ReflectionError> {
        validate_header(&words)?;
        let instructions = scan_instructions(&words)?;
        let parsed = ParsedModule::parse(&words, &instructions)?;
        let reflection = parsed.build_reflection()?;
        let stripped_words = parsed.strip_and_normalize(&words, &instructions)?;
        Ok(Self {
            raw_words: words,
            stripped_words,
            reflection,
        })
    }
}

#[derive(Debug)]
pub enum ReflectionError {
    Malformed(String),
    Unsupported(String),
    Inconsistent(String),
    LimitExceeded {
        item: &'static str,
        actual: u64,
        maximum: u64,
    },
    UnsupportedReflectionOpcode(u32),
    UnsupportedMemoryAccess {
        opcode: u16,
        mask: u32,
    },
}

impl fmt::Display for ReflectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(formatter, "malformed SPIR-V: {detail}"),
            Self::Unsupported(detail) => write!(formatter, "unsupported SPIR-V: {detail}"),
            Self::Inconsistent(detail) => write!(formatter, "inconsistent reflection: {detail}"),
            Self::LimitExceeded {
                item,
                actual,
                maximum,
            } => write!(formatter, "{item} count/size {actual} exceeds {maximum}"),
            Self::UnsupportedReflectionOpcode(opcode) => {
                write!(formatter, "unsupported ClspvReflection.5 opcode {opcode}")
            }
            Self::UnsupportedMemoryAccess { opcode, mask } => write!(
                formatter,
                "SPIR-V opcode {opcode} has unsupported Memory Access mask {mask:#x}"
            ),
        }
    }
}

impl Error for ReflectionError {}

#[derive(Clone, Copy)]
struct Instruction {
    offset: usize,
    word_count: usize,
    opcode: u16,
}

impl Instruction {
    fn operands<'a>(&self, words: &'a [u32]) -> &'a [u32] {
        &words[self.offset + 1..self.offset + self.word_count]
    }
}

fn validate_header(words: &[u32]) -> Result<(), ReflectionError> {
    if words.len() < HEADER_WORDS {
        return Err(ReflectionError::Malformed(
            "SPIR-V header is truncated".into(),
        ));
    }
    if words[0] != SPIRV_MAGIC {
        return Err(ReflectionError::Malformed(
            "little-endian SPIR-V magic is missing".into(),
        ));
    }
    if words[1] != SPIRV_VERSION_1_0 {
        return Err(ReflectionError::Unsupported(format!(
            "SPIR-V version word {:#x}; expected 1.0",
            words[1]
        )));
    }
    if words[3] == 0 || words[3] > MAX_ID_BOUND {
        return Err(ReflectionError::LimitExceeded {
            item: "SPIR-V ID bound",
            actual: u64::from(words[3]),
            maximum: u64::from(MAX_ID_BOUND),
        });
    }
    if words[4] != 0 {
        return Err(ReflectionError::Malformed(
            "reserved SPIR-V schema word is nonzero".into(),
        ));
    }
    Ok(())
}

fn scan_instructions(words: &[u32]) -> Result<Vec<Instruction>, ReflectionError> {
    let mut instructions = Vec::new();
    let mut offset = HEADER_WORDS;
    while offset < words.len() {
        if instructions.len() >= MAX_INSTRUCTIONS {
            return Err(ReflectionError::LimitExceeded {
                item: "SPIR-V instructions",
                actual: instructions.len() as u64 + 1,
                maximum: MAX_INSTRUCTIONS as u64,
            });
        }
        let first = words[offset];
        let word_count = (first >> 16) as usize;
        let opcode = (first & 0xffff) as u16;
        if word_count == 0 {
            return Err(ReflectionError::Malformed(format!(
                "instruction at word {offset} has zero word count"
            )));
        }
        let end = offset
            .checked_add(word_count)
            .ok_or_else(|| ReflectionError::Malformed("instruction word count overflow".into()))?;
        if end > words.len() {
            return Err(ReflectionError::Malformed(format!(
                "instruction at word {offset} overruns the module"
            )));
        }
        instructions.push(Instruction {
            offset,
            word_count,
            opcode,
        });
        offset = end;
    }
    Ok(instructions)
}

#[derive(Clone)]
struct EntryPoint {
    function_id: u32,
    name: String,
}

#[derive(Default)]
struct Decorations {
    descriptor_set: Option<u32>,
    binding: Option<u32>,
    spec_id: Option<u32>,
}

struct ReflectionRecord {
    result_type: u32,
    result_id: u32,
    set_id: u32,
    instruction: u32,
    operands: Vec<u32>,
}

struct ParsedModule {
    bound: u32,
    reflection_import: u32,
    strings: BTreeMap<u32, String>,
    void_types: BTreeSet<u32>,
    constants: BTreeMap<u32, u32>,
    spec_constant_ids: BTreeSet<u32>,
    entries: Vec<EntryPoint>,
    decorations: BTreeMap<u32, Decorations>,
    variables: BTreeMap<u32, u32>,
    execution_local_sizes: BTreeMap<u32, [u32; 3]>,
    reflection_records: Vec<ReflectionRecord>,
}

impl ParsedModule {
    fn parse(words: &[u32], instructions: &[Instruction]) -> Result<Self, ReflectionError> {
        let bound = words[3];
        let mut extensions = BTreeSet::new();
        let mut imports = BTreeMap::new();
        let mut strings = BTreeMap::new();
        let mut void_types = BTreeSet::new();
        let mut uint_types = BTreeSet::new();
        let mut constants = BTreeMap::new();
        let mut spec_constant_ids = BTreeSet::new();
        let mut entries = Vec::new();
        let mut functions = BTreeSet::new();
        let mut decorations: BTreeMap<u32, Decorations> = BTreeMap::new();
        let mut variables = BTreeMap::new();
        let mut execution_local_sizes = BTreeMap::new();
        let mut reflection_records = Vec::new();
        let mut memory_model_seen = false;

        for instruction in instructions {
            let operands = instruction.operands(words);
            match instruction.opcode {
                17 => {
                    expect_len(instruction, operands, 1)?;
                    if operands[0] != CAPABILITY_SHADER {
                        return Err(ReflectionError::Unsupported(format!(
                            "capability {} is outside the Shader-only contract",
                            operands[0]
                        )));
                    }
                }
                OP_EXTENSION => {
                    let (name, consumed) = decode_literal_string(operands)?;
                    if consumed != operands.len() {
                        return Err(ReflectionError::Malformed(
                            "OpExtension contains trailing operands".into(),
                        ));
                    }
                    if name != EXT_STORAGE_BUFFER && name != EXT_NONSEMANTIC {
                        return Err(ReflectionError::Unsupported(format!(
                            "extension {name:?} is not allowlisted"
                        )));
                    }
                    if !extensions.insert(name.clone()) {
                        return Err(ReflectionError::Inconsistent(format!(
                            "duplicate extension {name:?}"
                        )));
                    }
                }
                OP_EXT_INST_IMPORT => {
                    if operands.len() < 2 {
                        return Err(instruction_error(instruction, "truncated OpExtInstImport"));
                    }
                    check_id(operands[0], bound, "extended instruction import result")?;
                    let (name, consumed) = decode_literal_string(&operands[1..])?;
                    if consumed + 1 != operands.len() {
                        return Err(instruction_error(
                            instruction,
                            "OpExtInstImport contains trailing operands",
                        ));
                    }
                    if name != EXT_GLSL_STD_450 && name != CLSPV_REFLECTION_IMPORT {
                        return Err(ReflectionError::Unsupported(format!(
                            "extended instruction set {name:?} is not allowlisted"
                        )));
                    }
                    insert_unique(
                        &mut imports,
                        operands[0],
                        name,
                        "extended instruction import",
                    )?;
                }
                OP_MEMORY_MODEL => {
                    expect_len(instruction, operands, 2)?;
                    if memory_model_seen {
                        return Err(ReflectionError::Inconsistent(
                            "multiple OpMemoryModel instructions".into(),
                        ));
                    }
                    if operands != [ADDRESSING_MODEL_LOGICAL, MEMORY_MODEL_GLSL450] {
                        return Err(ReflectionError::Unsupported(format!(
                            "memory model operands {operands:?}; expected Logical GLSL450"
                        )));
                    }
                    memory_model_seen = true;
                }
                OP_ENTRY_POINT => {
                    if operands.len() < 3 {
                        return Err(instruction_error(instruction, "truncated OpEntryPoint"));
                    }
                    if operands[0] != EXECUTION_MODEL_GL_COMPUTE {
                        return Err(ReflectionError::Unsupported(format!(
                            "execution model {} is not GLCompute",
                            operands[0]
                        )));
                    }
                    check_id(operands[1], bound, "entry point function")?;
                    let (name, _) = decode_literal_string(&operands[2..])?;
                    if name.is_empty() {
                        return Err(ReflectionError::Malformed(
                            "compute entry point name is empty".into(),
                        ));
                    }
                    if entries.iter().any(|entry: &EntryPoint| entry.name == name) {
                        return Err(ReflectionError::Inconsistent(format!(
                            "duplicate entry point name {name:?}"
                        )));
                    }
                    entries.push(EntryPoint {
                        function_id: operands[1],
                        name,
                    });
                }
                OP_EXECUTION_MODE => {
                    expect_len(instruction, operands, 5)?;
                    if operands[1] != EXECUTION_MODE_LOCAL_SIZE {
                        return Err(ReflectionError::Unsupported(format!(
                            "execution mode {} is not LocalSize",
                            operands[1]
                        )));
                    }
                    let size = [operands[2], operands[3], operands[4]];
                    if size.contains(&0) {
                        return Err(ReflectionError::Malformed(
                            "LocalSize contains a zero dimension".into(),
                        ));
                    }
                    if execution_local_sizes.insert(operands[0], size).is_some() {
                        return Err(ReflectionError::Inconsistent(format!(
                            "entry function {} has duplicate LocalSize",
                            operands[0]
                        )));
                    }
                }
                OP_STRING => {
                    if operands.len() < 2 {
                        return Err(instruction_error(instruction, "truncated OpString"));
                    }
                    check_id(operands[0], bound, "OpString result")?;
                    let (value, consumed) = decode_literal_string(&operands[1..])?;
                    if consumed + 1 != operands.len() {
                        return Err(instruction_error(
                            instruction,
                            "OpString contains trailing operands",
                        ));
                    }
                    insert_unique(&mut strings, operands[0], value, "OpString")?;
                }
                OP_TYPE_VOID => {
                    expect_len(instruction, operands, 1)?;
                    check_id(operands[0], bound, "OpTypeVoid result")?;
                    if !void_types.insert(operands[0]) {
                        return Err(ReflectionError::Inconsistent(format!(
                            "duplicate void type ID {}",
                            operands[0]
                        )));
                    }
                }
                OP_TYPE_INT => {
                    expect_len(instruction, operands, 3)?;
                    check_id(operands[0], bound, "OpTypeInt result")?;
                    if operands[1] == 32 && operands[2] == 0 {
                        uint_types.insert(operands[0]);
                    }
                }
                OP_CONSTANT => {
                    expect_len(instruction, operands, 3)?;
                    check_id(operands[1], bound, "OpConstant result")?;
                    if !uint_types.contains(&operands[0]) {
                        continue;
                    }
                    insert_unique(&mut constants, operands[1], operands[2], "uint constant")?;
                }
                OP_SPEC_CONSTANT => {
                    expect_len(instruction, operands, 3)?;
                    check_id(operands[1], bound, "OpSpecConstant result")?;
                    spec_constant_ids.insert(operands[1]);
                }
                OP_FUNCTION => {
                    if operands.len() < 4 {
                        return Err(instruction_error(instruction, "truncated OpFunction"));
                    }
                    check_id(operands[1], bound, "OpFunction result")?;
                    if !functions.insert(operands[1]) {
                        return Err(ReflectionError::Inconsistent(format!(
                            "duplicate function ID {}",
                            operands[1]
                        )));
                    }
                }
                OP_DECORATE => {
                    if operands.len() < 2 {
                        return Err(instruction_error(instruction, "truncated OpDecorate"));
                    }
                    check_id(operands[0], bound, "decoration target")?;
                    let target = decorations.entry(operands[0]).or_default();
                    match operands[1] {
                        DECORATION_SPEC_ID => {
                            expect_len(instruction, operands, 3)?;
                            set_once(&mut target.spec_id, operands[2], "SpecId", operands[0])?;
                        }
                        DECORATION_BINDING => {
                            expect_len(instruction, operands, 3)?;
                            set_once(&mut target.binding, operands[2], "Binding", operands[0])?;
                        }
                        DECORATION_DESCRIPTOR_SET => {
                            expect_len(instruction, operands, 3)?;
                            set_once(
                                &mut target.descriptor_set,
                                operands[2],
                                "DescriptorSet",
                                operands[0],
                            )?;
                        }
                        _ => {}
                    }
                }
                OP_VARIABLE => {
                    if !(3..=4).contains(&operands.len()) {
                        return Err(instruction_error(
                            instruction,
                            "OpVariable has an invalid operand count",
                        ));
                    }
                    check_id(operands[1], bound, "OpVariable result")?;
                    insert_unique(&mut variables, operands[1], operands[2], "global variable")?;
                }
                OP_EXT_INST => {
                    if operands.len() < 4 {
                        return Err(instruction_error(instruction, "truncated OpExtInst"));
                    }
                    check_id(operands[1], bound, "OpExtInst result")?;
                    reflection_records.push(ReflectionRecord {
                        result_type: operands[0],
                        result_id: operands[1],
                        set_id: operands[2],
                        instruction: operands[3],
                        operands: operands[4..].to_vec(),
                    });
                }
                OP_LOAD | OP_STORE => validate_memory_operands(instruction, operands)?,
                331 => {
                    return Err(ReflectionError::Unsupported(
                        "OpExecutionModeId is outside the bounded ABI".into(),
                    ));
                }
                _ => {}
            }
        }

        if !memory_model_seen {
            return Err(ReflectionError::Malformed("missing OpMemoryModel".into()));
        }
        if !extensions.contains(EXT_STORAGE_BUFFER) {
            return Err(ReflectionError::Inconsistent(format!(
                "missing required {EXT_STORAGE_BUFFER} extension"
            )));
        }
        if !extensions.contains(EXT_NONSEMANTIC) {
            return Err(ReflectionError::Inconsistent(format!(
                "missing required {EXT_NONSEMANTIC} extension"
            )));
        }
        let reflection_imports = imports
            .iter()
            .filter_map(|(&id, name)| (name == CLSPV_REFLECTION_IMPORT).then_some(id))
            .collect::<Vec<_>>();
        let reflection_import = match reflection_imports.as_slice() {
            [id] => *id,
            [] => {
                return Err(ReflectionError::Inconsistent(
                    "missing NonSemantic.ClspvReflection.5 import".into(),
                ));
            }
            _ => {
                return Err(ReflectionError::Inconsistent(
                    "multiple ClspvReflection.5 imports".into(),
                ));
            }
        };
        for record in &reflection_records {
            // Non-reflection ExtInst instructions (for example GLSL.std.450)
            // are retained, but their import ID must have been declared.
            if !imports.contains_key(&record.set_id) {
                return Err(ReflectionError::Malformed(format!(
                    "OpExtInst result {} references unknown import {}",
                    record.result_id, record.set_id
                )));
            }
        }
        if entries.is_empty() {
            return Err(ReflectionError::Inconsistent(
                "module has no GLCompute entry points".into(),
            ));
        }
        if entries.len() > MAX_KERNELS {
            return Err(ReflectionError::LimitExceeded {
                item: "compute entry points",
                actual: entries.len() as u64,
                maximum: MAX_KERNELS as u64,
            });
        }
        for entry in &entries {
            if !functions.contains(&entry.function_id) {
                return Err(ReflectionError::Malformed(format!(
                    "entry point {:?} references missing function {}",
                    entry.name, entry.function_id
                )));
            }
        }

        Ok(Self {
            bound,
            reflection_import,
            strings,
            void_types,
            constants,
            spec_constant_ids,
            entries,
            decorations,
            variables,
            execution_local_sizes,
            reflection_records,
        })
    }

    fn build_reflection(&self) -> Result<SpirvReflection, ReflectionError> {
        let mut kernels: BTreeMap<u32, KernelBuilder> = BTreeMap::new();
        let mut argument_info: BTreeMap<u32, String> = BTreeMap::new();
        let mut workgroup_spec_ids = None;
        let mut work_dim_spec_id = None;

        for record in self
            .reflection_records
            .iter()
            .filter(|record| record.set_id == self.reflection_import)
        {
            if !self.void_types.contains(&record.result_type) {
                return Err(ReflectionError::Malformed(format!(
                    "reflection result {} does not use OpTypeVoid",
                    record.result_id
                )));
            }
            check_id(record.result_id, self.bound, "reflection result")?;
            match record.instruction {
                1 => {
                    if record.operands.len() != 5 {
                        return Err(ReflectionError::Malformed(
                            "Kernel reflection record must have five operands".into(),
                        ));
                    }
                    let function_id = record.operands[0];
                    let name = self.string(record.operands[1], "kernel name")?.to_owned();
                    let declared_argument_count =
                        self.constant(record.operands[2], "kernel argument count")?;
                    let flags = self.constant(record.operands[3], "kernel flags")?;
                    let attributes = self.string(record.operands[4], "kernel attributes")?;
                    if flags != 0 {
                        return Err(ReflectionError::Unsupported(format!(
                            "kernel {name:?} has reflection flags {flags:#x}"
                        )));
                    }
                    if attributes != "__kernel" && !attributes.is_empty() {
                        return Err(ReflectionError::Unsupported(format!(
                            "kernel {name:?} has attributes {attributes:?}"
                        )));
                    }
                    if declared_argument_count > MAX_ARGUMENTS_PER_KERNEL {
                        return Err(ReflectionError::LimitExceeded {
                            item: "kernel arguments",
                            actual: u64::from(declared_argument_count),
                            maximum: u64::from(MAX_ARGUMENTS_PER_KERNEL),
                        });
                    }
                    let entry = self
                        .entries
                        .iter()
                        .find(|entry| entry.function_id == function_id)
                        .ok_or_else(|| {
                            ReflectionError::Inconsistent(format!(
                                "reflected kernel {name:?} has no GLCompute entry point"
                            ))
                        })?;
                    if entry.name != name {
                        return Err(ReflectionError::Inconsistent(format!(
                            "kernel reflection name {name:?} differs from entry point {:?}",
                            entry.name
                        )));
                    }
                    if kernels
                        .insert(
                            record.result_id,
                            KernelBuilder {
                                name,
                                function_id,
                                declared_argument_count,
                                arguments: Vec::new(),
                                required_workgroup_size: None,
                            },
                        )
                        .is_some()
                    {
                        return Err(ReflectionError::Inconsistent(format!(
                            "duplicate kernel declaration ID {}",
                            record.result_id
                        )));
                    }
                }
                2 => {
                    if !(1..=5).contains(&record.operands.len()) {
                        return Err(ReflectionError::Malformed(
                            "ArgumentInfo has an invalid operand count".into(),
                        ));
                    }
                    let name = self.string(record.operands[0], "argument name")?.to_owned();
                    if record.operands.len() >= 2 {
                        self.string(record.operands[1], "argument type name")?;
                    }
                    for qualifier in record.operands.iter().skip(2) {
                        self.constant(*qualifier, "argument qualifier")?;
                    }
                    insert_unique(&mut argument_info, record.result_id, name, "ArgumentInfo")?;
                }
                3 => {
                    if !(4..=5).contains(&record.operands.len()) {
                        return Err(ReflectionError::Malformed(
                            "ArgumentStorageBuffer has an invalid operand count".into(),
                        ));
                    }
                    let argument = self.resource_argument(
                        record,
                        &argument_info,
                        KernelArgumentKind::StorageBuffer,
                    )?;
                    self.kernel_mut(&mut kernels, record.operands[0])?
                        .arguments
                        .push(argument);
                }
                6 => {
                    if !(6..=7).contains(&record.operands.len()) {
                        return Err(ReflectionError::Malformed(
                            "ArgumentPodUniform has an invalid operand count".into(),
                        ));
                    }
                    let offset = self.constant(record.operands[4], "POD offset")?;
                    let size = self.constant(record.operands[5], "POD size")?;
                    let end = offset
                        .checked_add(size)
                        .filter(|_| size != 0)
                        .ok_or_else(|| {
                            ReflectionError::Inconsistent(format!(
                                "POD range {offset}+{size} is invalid"
                            ))
                        })?;
                    if end > MAX_POD_BYTES {
                        return Err(ReflectionError::LimitExceeded {
                            item: "POD uniform span",
                            actual: u64::from(end),
                            maximum: u64::from(MAX_POD_BYTES),
                        });
                    }
                    let argument = self.resource_argument(
                        record,
                        &argument_info,
                        KernelArgumentKind::PodUniform { offset, size },
                    )?;
                    self.kernel_mut(&mut kernels, record.operands[0])?
                        .arguments
                        .push(argument);
                }
                12 => {
                    if record.operands.len() != 3 {
                        return Err(ReflectionError::Malformed(
                            "SpecConstantWorkgroupSize must have three operands".into(),
                        ));
                    }
                    let ids = WorkgroupSpecIds {
                        x: self.constant(record.operands[0], "workgroup X SpecId")?,
                        y: self.constant(record.operands[1], "workgroup Y SpecId")?,
                        z: self.constant(record.operands[2], "workgroup Z SpecId")?,
                    };
                    if ids.x == ids.y || ids.x == ids.z || ids.y == ids.z {
                        return Err(ReflectionError::Inconsistent(
                            "workgroup SpecIds are not distinct".into(),
                        ));
                    }
                    if workgroup_spec_ids.replace(ids).is_some() {
                        return Err(ReflectionError::Inconsistent(
                            "duplicate SpecConstantWorkgroupSize record".into(),
                        ));
                    }
                }
                14 => {
                    if record.operands.len() != 1 {
                        return Err(ReflectionError::Malformed(
                            "SpecConstantWorkDim must have one operand".into(),
                        ));
                    }
                    let id = self.constant(record.operands[0], "work_dim SpecId")?;
                    if work_dim_spec_id.replace(id).is_some() {
                        return Err(ReflectionError::Inconsistent(
                            "duplicate SpecConstantWorkDim record".into(),
                        ));
                    }
                }
                24 => {
                    if record.operands.len() != 4 {
                        return Err(ReflectionError::Malformed(
                            "PropertyRequiredWorkgroupSize must have four operands".into(),
                        ));
                    }
                    let size = [
                        self.constant(record.operands[1], "required workgroup X")?,
                        self.constant(record.operands[2], "required workgroup Y")?,
                        self.constant(record.operands[3], "required workgroup Z")?,
                    ];
                    if size.contains(&0) {
                        return Err(ReflectionError::Inconsistent(
                            "required workgroup size contains zero".into(),
                        ));
                    }
                    let kernel = self.kernel_mut(&mut kernels, record.operands[0])?;
                    if kernel.required_workgroup_size.replace(size).is_some() {
                        return Err(ReflectionError::Inconsistent(format!(
                            "kernel {:?} has duplicate required workgroup size",
                            kernel.name
                        )));
                    }
                }
                opcode => return Err(ReflectionError::UnsupportedReflectionOpcode(opcode)),
            }
        }

        if kernels.len() != self.entries.len() {
            return Err(ReflectionError::Inconsistent(format!(
                "{} reflected kernels do not exactly cover {} entry points",
                kernels.len(),
                self.entries.len()
            )));
        }
        if kernels.len() > MAX_KERNELS {
            return Err(ReflectionError::LimitExceeded {
                item: "reflected kernels",
                actual: kernels.len() as u64,
                maximum: MAX_KERNELS as u64,
            });
        }

        let mut finalized = Vec::with_capacity(kernels.len());
        for (_, mut kernel) in kernels {
            kernel.arguments.sort_by_key(|argument| argument.ordinal);
            if kernel.arguments.len() != kernel.declared_argument_count as usize {
                return Err(ReflectionError::Inconsistent(format!(
                    "kernel {:?} declares {} arguments but has {} supported records",
                    kernel.name,
                    kernel.declared_argument_count,
                    kernel.arguments.len()
                )));
            }
            for (expected, argument) in kernel.arguments.iter().enumerate() {
                if argument.ordinal != expected as u32 {
                    return Err(ReflectionError::Inconsistent(format!(
                        "kernel {:?} has missing/duplicate ordinal {}; saw {}",
                        kernel.name, expected, argument.ordinal
                    )));
                }
            }
            validate_kernel_bindings(&kernel)?;
            if let Some(required) = kernel.required_workgroup_size {
                if self.execution_local_sizes.get(&kernel.function_id) != Some(&required) {
                    return Err(ReflectionError::Inconsistent(format!(
                        "kernel {:?} required size {required:?} does not match LocalSize",
                        kernel.name
                    )));
                }
            } else if self.execution_local_sizes.contains_key(&kernel.function_id) {
                return Err(ReflectionError::Inconsistent(format!(
                    "kernel {:?} has LocalSize without required-size reflection",
                    kernel.name
                )));
            }
            finalized.push(KernelReflection {
                name: kernel.name,
                function_id: kernel.function_id,
                declared_argument_count: kernel.declared_argument_count,
                arguments: kernel.arguments,
                required_workgroup_size: kernel.required_workgroup_size,
            });
        }
        finalized.sort_by(|left, right| left.name.cmp(&right.name));

        self.validate_spec_ids(workgroup_spec_ids, work_dim_spec_id)?;
        self.validate_descriptors(&finalized)?;

        Ok(SpirvReflection {
            kernels: finalized,
            workgroup_spec_ids,
            work_dim_spec_id,
        })
    }

    fn resource_argument(
        &self,
        record: &ReflectionRecord,
        argument_info: &BTreeMap<u32, String>,
        kind: KernelArgumentKind,
    ) -> Result<KernelArgument, ReflectionError> {
        let ordinal = self.constant(record.operands[1], "argument ordinal")?;
        let descriptor_set = self.constant(record.operands[2], "descriptor set")?;
        let binding = self.constant(record.operands[3], "binding")?;
        if descriptor_set > MAX_DESCRIPTOR_SET {
            return Err(ReflectionError::LimitExceeded {
                item: "descriptor set",
                actual: u64::from(descriptor_set),
                maximum: u64::from(MAX_DESCRIPTOR_SET),
            });
        }
        if binding > MAX_BINDING {
            return Err(ReflectionError::LimitExceeded {
                item: "descriptor binding",
                actual: u64::from(binding),
                maximum: u64::from(MAX_BINDING),
            });
        }
        let info_index = match kind {
            KernelArgumentKind::StorageBuffer => 4,
            KernelArgumentKind::PodUniform { .. } => 6,
        };
        let name = record
            .operands
            .get(info_index)
            .map(|id| {
                argument_info.get(id).cloned().ok_or_else(|| {
                    ReflectionError::Malformed(format!(
                        "argument references missing ArgumentInfo ID {id}"
                    ))
                })
            })
            .transpose()?;
        Ok(KernelArgument {
            ordinal,
            descriptor_set,
            binding,
            name,
            kind,
        })
    }

    fn kernel_mut<'a>(
        &self,
        kernels: &'a mut BTreeMap<u32, KernelBuilder>,
        declaration: u32,
    ) -> Result<&'a mut KernelBuilder, ReflectionError> {
        kernels.get_mut(&declaration).ok_or_else(|| {
            ReflectionError::Malformed(format!(
                "argument/property references missing Kernel record {declaration}"
            ))
        })
    }

    fn constant(&self, id: u32, purpose: &str) -> Result<u32, ReflectionError> {
        self.constants.get(&id).copied().ok_or_else(|| {
            ReflectionError::Malformed(format!("{purpose} references non-uint OpConstant ID {id}"))
        })
    }

    fn string<'a>(&'a self, id: u32, purpose: &str) -> Result<&'a str, ReflectionError> {
        self.strings.get(&id).map(String::as_str).ok_or_else(|| {
            ReflectionError::Malformed(format!("{purpose} references missing OpString ID {id}"))
        })
    }

    fn validate_spec_ids(
        &self,
        workgroup: Option<WorkgroupSpecIds>,
        work_dim: Option<u32>,
    ) -> Result<(), ReflectionError> {
        let mut expected = BTreeSet::new();
        if let Some(ids) = workgroup {
            expected.extend([ids.x, ids.y, ids.z]);
        }
        if let Some(id) = work_dim {
            if !expected.insert(id) {
                return Err(ReflectionError::Inconsistent(format!(
                    "work_dim SpecId {id} overlaps a workgroup SpecId"
                )));
            }
        }
        let mut actual = BTreeMap::<u32, usize>::new();
        for (&target, decoration) in &self.decorations {
            if let Some(spec_id) = decoration.spec_id {
                if !self.spec_constant_ids.contains(&target) {
                    return Err(ReflectionError::Malformed(format!(
                        "SpecId {spec_id} decorates non-OpSpecConstant ID {target}"
                    )));
                }
                *actual.entry(spec_id).or_default() += 1;
            }
        }
        for spec_id in expected {
            match actual.get(&spec_id).copied().unwrap_or(0) {
                1 => {}
                count => {
                    return Err(ReflectionError::Inconsistent(format!(
                        "reflected SpecId {spec_id} has {count} semantic declarations"
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_descriptors(&self, kernels: &[KernelReflection]) -> Result<(), ReflectionError> {
        let mut expected = BTreeMap::<(u32, u32), DescriptorKind>::new();
        for kernel in kernels {
            for argument in &kernel.arguments {
                let key = (argument.descriptor_set, argument.binding);
                let kind = match argument.kind {
                    KernelArgumentKind::StorageBuffer => DescriptorKind::Storage,
                    KernelArgumentKind::PodUniform { .. } => DescriptorKind::Uniform,
                };
                if let Some(previous) = expected.insert(key, kind)
                    && previous != kind
                {
                    return Err(ReflectionError::Inconsistent(format!(
                        "descriptor {}:{} has conflicting reflected kinds",
                        key.0, key.1
                    )));
                }
            }
        }

        let mut actual = BTreeMap::<(u32, u32), Vec<DescriptorKind>>::new();
        for (&id, &storage_class) in &self.variables {
            let decoration = self.decorations.get(&id);
            let set = decoration.and_then(|value| value.descriptor_set);
            let binding = decoration.and_then(|value| value.binding);
            if set.is_some() != binding.is_some() {
                return Err(ReflectionError::Malformed(format!(
                    "resource variable {id} has only one of DescriptorSet/Binding"
                )));
            }
            let Some(key) = set.zip(binding) else {
                continue;
            };
            let kind = match storage_class {
                STORAGE_CLASS_STORAGE_BUFFER => DescriptorKind::Storage,
                STORAGE_CLASS_UNIFORM => DescriptorKind::Uniform,
                other => {
                    return Err(ReflectionError::Unsupported(format!(
                        "bound variable {id} has storage class {other}"
                    )));
                }
            };
            actual.entry(key).or_default().push(kind);
        }
        for (&key, kinds) in &actual {
            let expected_kind = expected.get(&key).ok_or_else(|| {
                ReflectionError::Inconsistent(format!(
                    "semantic descriptor {}:{} has no reflection record",
                    key.0, key.1
                ))
            })?;
            if kinds.iter().any(|kind| kind != expected_kind) {
                return Err(ReflectionError::Inconsistent(format!(
                    "semantic descriptor {}:{} has the wrong storage class",
                    key.0, key.1
                )));
            }
        }
        for (&key, kind) in &expected {
            if !actual
                .get(&key)
                .is_some_and(|values| values.iter().any(|value| value == kind))
            {
                return Err(ReflectionError::Inconsistent(format!(
                    "reflected descriptor {}:{} is missing from semantic SPIR-V",
                    key.0, key.1
                )));
            }
        }
        Ok(())
    }

    fn strip_and_normalize(
        &self,
        words: &[u32],
        instructions: &[Instruction],
    ) -> Result<Vec<u32>, ReflectionError> {
        let mut output = Vec::with_capacity(words.len());
        output.extend_from_slice(&words[..HEADER_WORDS]);
        for instruction in instructions {
            let operands = instruction.operands(words);
            let remove = match instruction.opcode {
                OP_EXTENSION => decode_literal_string(operands)?.0 == EXT_NONSEMANTIC,
                OP_EXT_INST_IMPORT => operands.first() == Some(&self.reflection_import),
                OP_EXT_INST => operands.get(2) == Some(&self.reflection_import),
                _ => false,
            };
            if remove {
                continue;
            }
            match instruction.opcode {
                OP_LOAD => append_normalized_memory_instruction(
                    &mut output,
                    instruction.opcode,
                    operands,
                    3,
                )?,
                OP_STORE => append_normalized_memory_instruction(
                    &mut output,
                    instruction.opcode,
                    operands,
                    2,
                )?,
                _ => output.extend_from_slice(
                    &words[instruction.offset..instruction.offset + instruction.word_count],
                ),
            }
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DescriptorKind {
    Storage,
    Uniform,
}

struct KernelBuilder {
    name: String,
    function_id: u32,
    declared_argument_count: u32,
    arguments: Vec<KernelArgument>,
    required_workgroup_size: Option<[u32; 3]>,
}

fn validate_kernel_bindings(kernel: &KernelBuilder) -> Result<(), ReflectionError> {
    let mut non_pod = BTreeSet::new();
    let mut pod_binding = None;
    let mut pod_ranges = Vec::new();
    for argument in &kernel.arguments {
        let key = (argument.descriptor_set, argument.binding);
        match argument.kind {
            KernelArgumentKind::StorageBuffer => {
                if !non_pod.insert(key) || pod_binding == Some(key) {
                    return Err(ReflectionError::Inconsistent(format!(
                        "kernel {:?} duplicates descriptor {}:{} outside clustered POD",
                        kernel.name, key.0, key.1
                    )));
                }
            }
            KernelArgumentKind::PodUniform { offset, size } => {
                if non_pod.contains(&key) {
                    return Err(ReflectionError::Inconsistent(format!(
                        "kernel {:?} aliases POD and storage descriptor {}:{}",
                        kernel.name, key.0, key.1
                    )));
                }
                if let Some(previous) = pod_binding {
                    if previous != key {
                        return Err(ReflectionError::Inconsistent(format!(
                            "kernel {:?} has more than one clustered POD binding",
                            kernel.name
                        )));
                    }
                } else {
                    pod_binding = Some(key);
                }
                pod_ranges.push((offset, offset + size));
            }
        }
    }
    pod_ranges.sort_unstable();
    if pod_ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(ReflectionError::Inconsistent(format!(
            "kernel {:?} has overlapping POD ranges",
            kernel.name
        )));
    }
    Ok(())
}

fn validate_memory_operands(
    instruction: &Instruction,
    operands: &[u32],
) -> Result<(), ReflectionError> {
    let base = if instruction.opcode == OP_LOAD { 3 } else { 2 };
    match operands.len() {
        length if length == base => Ok(()),
        length if length == base + 2 => {
            let mask = operands[base];
            let alignment = operands[base + 1];
            if mask != MEMORY_ACCESS_ALIGNED {
                return Err(ReflectionError::UnsupportedMemoryAccess {
                    opcode: instruction.opcode,
                    mask,
                });
            }
            if alignment == 0 || !alignment.is_power_of_two() {
                return Err(ReflectionError::Malformed(format!(
                    "opcode {} has invalid alignment {alignment}",
                    instruction.opcode
                )));
            }
            Ok(())
        }
        _ => Err(instruction_error(
            instruction,
            "unsupported Memory Access operand shape",
        )),
    }
}

fn append_normalized_memory_instruction(
    output: &mut Vec<u32>,
    opcode: u16,
    operands: &[u32],
    base: usize,
) -> Result<(), ReflectionError> {
    let instruction = Instruction {
        offset: 0,
        word_count: operands.len() + 1,
        opcode,
    };
    validate_memory_operands(&instruction, operands)?;
    output.push((((base + 1) as u32) << 16) | u32::from(opcode));
    output.extend_from_slice(&operands[..base]);
    Ok(())
}

fn decode_literal_string(words: &[u32]) -> Result<(String, usize), ReflectionError> {
    let mut bytes = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let encoded = word.to_le_bytes();
        if let Some(nul) = encoded.iter().position(|byte| *byte == 0) {
            if encoded[nul + 1..].iter().any(|byte| *byte != 0) {
                return Err(ReflectionError::Malformed(
                    "literal string has nonzero padding after NUL".into(),
                ));
            }
            bytes.extend_from_slice(&encoded[..nul]);
            if bytes.len() > MAX_STRING_BYTES {
                return Err(ReflectionError::LimitExceeded {
                    item: "SPIR-V literal string bytes",
                    actual: bytes.len() as u64,
                    maximum: MAX_STRING_BYTES as u64,
                });
            }
            let value = String::from_utf8(bytes).map_err(|error| {
                ReflectionError::Malformed(format!("literal string is not UTF-8: {error}"))
            })?;
            return Ok((value, index + 1));
        }
        bytes.extend_from_slice(&encoded);
        if bytes.len() > MAX_STRING_BYTES {
            return Err(ReflectionError::LimitExceeded {
                item: "SPIR-V literal string bytes",
                actual: bytes.len() as u64,
                maximum: MAX_STRING_BYTES as u64,
            });
        }
    }
    Err(ReflectionError::Malformed(
        "literal string is not NUL-terminated".into(),
    ))
}

fn check_id(id: u32, bound: u32, purpose: &str) -> Result<(), ReflectionError> {
    if id == 0 || id >= bound {
        return Err(ReflectionError::Malformed(format!(
            "{purpose} ID {id} is outside 1..{bound}"
        )));
    }
    Ok(())
}

fn expect_len(
    instruction: &Instruction,
    operands: &[u32],
    expected: usize,
) -> Result<(), ReflectionError> {
    if operands.len() == expected {
        Ok(())
    } else {
        Err(instruction_error(
            instruction,
            &format!("expected {expected} operands, found {}", operands.len()),
        ))
    }
}

fn instruction_error(instruction: &Instruction, detail: &str) -> ReflectionError {
    ReflectionError::Malformed(format!(
        "opcode {} at word {}: {detail}",
        instruction.opcode, instruction.offset
    ))
}

fn insert_unique<K: Ord + fmt::Display, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    item: &str,
) -> Result<(), ReflectionError> {
    if map.insert(key, value).is_some() {
        return Err(ReflectionError::Inconsistent(format!(
            "duplicate {item} definition"
        )));
    }
    Ok(())
}

fn set_once(
    slot: &mut Option<u32>,
    value: u32,
    decoration: &str,
    target: u32,
) -> Result<(), ReflectionError> {
    if slot.replace(value).is_some() {
        return Err(ReflectionError::Inconsistent(format!(
            "ID {target} has duplicate {decoration} decorations"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_and_misaligned_modules() {
        assert!(matches!(
            SpirvModule::parse_bytes(&[0; 3]),
            Err(ReflectionError::Malformed(_))
        ));
        assert!(matches!(
            SpirvModule::parse_bytes(&SPIRV_MAGIC.to_le_bytes()),
            Err(ReflectionError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_zero_word_count_instruction() {
        let words = [SPIRV_MAGIC, SPIRV_VERSION_1_0, 0, 2, 0, 0];
        assert!(matches!(
            scan_instructions(&words),
            Err(ReflectionError::Malformed(_))
        ));
    }

    #[test]
    fn strips_only_an_aligned_memory_hint() {
        let instruction = Instruction {
            offset: 5,
            word_count: 6,
            opcode: OP_LOAD,
        };
        validate_memory_operands(&instruction, &[1, 2, 3, MEMORY_ACCESS_ALIGNED, 16]).unwrap();
        let mut output = Vec::new();
        append_normalized_memory_instruction(
            &mut output,
            OP_LOAD,
            &[1, 2, 3, MEMORY_ACCESS_ALIGNED, 16],
            3,
        )
        .unwrap();
        assert_eq!(output, vec![(4_u32 << 16) | u32::from(OP_LOAD), 1, 2, 3]);
    }

    #[test]
    fn rejects_unknown_memory_access_mask_and_bad_alignment() {
        let instruction = Instruction {
            offset: 5,
            word_count: 6,
            opcode: OP_LOAD,
        };
        assert!(matches!(
            validate_memory_operands(&instruction, &[1, 2, 3, 1, 16]),
            Err(ReflectionError::UnsupportedMemoryAccess { mask: 1, .. })
        ));
        assert!(matches!(
            validate_memory_operands(&instruction, &[1, 2, 3, MEMORY_ACCESS_ALIGNED, 3]),
            Err(ReflectionError::Malformed(_))
        ));
    }

    #[test]
    fn literal_string_requires_zero_padding_and_utf8() {
        assert_eq!(
            decode_literal_string(&[u32::from_le_bytes(*b"abc\0")]).unwrap(),
            ("abc".into(), 1)
        );
        assert!(matches!(
            decode_literal_string(&[u32::from_le_bytes(*b"a\0x\0")]),
            Err(ReflectionError::Malformed(_))
        ));
        assert!(matches!(
            decode_literal_string(&[u32::from_le_bytes([0xff, 0, 0, 0])]),
            Err(ReflectionError::Malformed(_))
        ));
    }
}
