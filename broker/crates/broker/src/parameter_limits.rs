//! Host resource budgets, not a claim about Adobe's maximum. The native host
//! already supports 4096 controls (excluding input slot zero); released effects
//! include 1623-control OLM Color Key and 1265-control Universe Text Tile.
pub const MAX_PARAMETERS: u32 = 4096;
/// Aggregate ASCII transport budget: 256 bytes per supported control. Individual
/// arbitrary text retains its 4096-byte bound and shares this aggregate budget.
pub const MAX_PARAMETER_PAYLOAD_BYTES: usize = MAX_PARAMETERS as usize * 256;
/// JSON may escape each byte as six characters, plus bounded context metadata.
pub const MAX_PARAMETER_DOCUMENT_BYTES: usize = MAX_PARAMETER_PAYLOAD_BYTES * 6 + 64 * 1024;
/// Payload syntax is ASCII without JSON quotes/backslashes; reserve framing space.
pub const MAX_PARAMETER_MESSAGE_BYTES: usize = MAX_PARAMETER_PAYLOAD_BYTES + 64 * 1024;
