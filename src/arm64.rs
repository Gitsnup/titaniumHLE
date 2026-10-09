//! Minimal AArch64 (ARM64) user-mode interpreter.
//!
//! This is the core of titaniumHLE's experimental ARM64 guest support. It is
//! deliberately tiny: it executes a small subset of A64 and stops with a clear
//! error for anything it does not understand. It is intended for very simple,
//! statically linked arm64 executables, not for real applications.

pub struct Cpu {
    pub x: [u64; 31],
    pub sp: u64,
    pub pc: u64,
    n: bool,
    z: bool,
    c: bool,
    v: bool,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            x: [0; 31],
            sp: 0,
            pc: 0,
            n: false,
            z: false,
            c: false,
            v: false,
        }
    }
}

/// What to do when the guest executes an SVC instruction.
pub enum SvcResult {
    Continue,
    /// The guest has requested process exit; carries the exit status.
    Halt(u64),
}

/// Memory and system-call interface used by the interpreter.
pub trait Backend {
    /// Read `buf.len()` bytes at `addr`. Returns false on a fault.
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> bool;
    /// Write `buf` at `addr`. Returns false on a fault.
    fn write(&mut self, addr: u32, buf: &[u8]) -> bool;
    /// Handle an SVC trap. The syscall number is in `x16`.
    fn syscall(&mut self, cpu: &mut Cpu) -> Result<SvcResult, String>;
}

/// Magic link register value that means "_main has returned".
pub const EXIT_MAGIC: u64 = 0xc0de_ba5e;

fn field(insn: u32, hi: u32, lo: u32) -> u32 {
    (insn >> lo) & ((1u32 << (hi - lo + 1)) - 1)
}

fn sext(value: u64, bits: u32) -> u64 {
    let shift = 64 - bits;
    ((value << shift) as i64 >> shift) as u64
}

fn sign_extend32(value: u32, bits: u32) -> i64 {
    (((value << (32 - bits)) as i32) >> (32 - bits)) as i64
}

fn cond_passes(cond: u32, cpu: &Cpu) -> Result<bool, String> {
    Ok(match cond {
        0 => cpu.z,
        1 => !cpu.z,
        2 => cpu.c,
        3 => !cpu.c,
        4 => cpu.n,
        5 => !cpu.n,
        6 => cpu.v,
        7 => !cpu.v,
        8 => cpu.c && !cpu.z,
        9 => !cpu.c || cpu.z,
        10 => cpu.n == cpu.v,
        11 => cpu.n != cpu.v,
        12 => !cpu.z && (cpu.n == cpu.v),
        13 => cpu.z || (cpu.n != cpu.v),
        14 => true,
        _ => return Err("unimplemented condition code NV".to_string()),
    })
}

fn rotate_right64(value: u64, amount: u64, width: u32) -> u64 {
    if width == 64 {
        value.rotate_right((amount % 64) as u32)
    } else {
        let mask = (1u64 << width) - 1;
        let v = value & mask;
        let a = (amount % width as u64) as u32;
        let w = width;
        ((v >> a) | ((v << (w - a)) & mask)) & mask
    }
}

fn decode_bitmask_imm(insn: u32) -> Option<u64> {
    let sf = field(insn, 31, 31);
    let n = field(insn, 22, 22);
    if sf == 0 && n == 1 {
        return None;
    }
    let immr = field(insn, 21, 16) as u64;
    let imms = field(insn, 15, 10) as u64;
    let not_imms = (!imms) & 0x3f;
    if not_imms == 0 {
        return None;
    }
    let len: u64 = if n == 1 {
        6
    } else {
        u64::from(31 - not_imms.leading_zeros())
    };
    let esize = 1u64 << len;
    let levels = esize - 1;
    let s = imms & levels;
    let r = immr & levels;
    if s == levels {
        return None;
    }
    let welem = rotate_right64((1u64 << (s + 1)) - 1, r, esize as u32);
    let size: u64 = if sf == 1 { 64 } else { 32 };
    let mut imm = 0u64;
    let mut i = 0u64;
    while i < size {
        imm |= welem << i;
        i += esize;
    }
    Some(if sf == 1 { imm } else { imm & 0xffff_ffff })
}

#[derive(Clone, Copy, PartialEq)]
enum RegWidth {
    W,
    X,
}

impl Cpu {
    fn read_reg(&self, i: u32, width: RegWidth) -> u64 {
        match width {
            RegWidth::X => {
                if i == 31 {
                    0
                } else {
                    self.x[i as usize]
                }
            }
            RegWidth::W => {
                if i == 31 {
                    0
                } else {
                    self.x[i as usize] & 0xffff_ffff
                }
            }
        }
    }

    fn write_reg(&mut self, i: u32, value: u64, width: RegWidth) {
        match width {
            RegWidth::X => {
                if i != 31 {
                    self.x[i as usize] = value;
                }
            }
            RegWidth::W => {
                if i != 31 {
                    self.x[i as usize] = value & 0xffff_ffff;
                }
            }
        }
    }

    // For add/sub immediate: register 31 means SP.
    fn read_sp_or_reg(&self, i: u32, width: RegWidth) -> u64 {
        if i == 31 {
            self.sp
        } else {
            self.read_reg(i, width)
        }
    }

    fn write_sp_or_reg(&mut self, i: u32, value: u64, width: RegWidth, sp_allowed: bool) {
        if i == 31 {
            if sp_allowed {
                self.sp = value;
            }
        } else {
            self.write_reg(i, value, width);
        }
    }

    fn set_flags(
        &mut self,
        a: u64,
        b: u64,
        result: u64,
        carry_out: u64,
        subtract: bool,
        width_bits: u32,
    ) {
        let sign_bit = 1u64 << (width_bits - 1);
        self.n = (result & sign_bit) != 0;
        self.z = result == 0;
        self.c = carry_out == 1;
        if subtract {
            self.v = ((a ^ b) & sign_bit) != 0 && ((result ^ a) & sign_bit) != 0;
        } else {
            self.v = ((a ^ result) & sign_bit) != 0 && ((b ^ result) & sign_bit) != 0;
        }
    }
}

fn add_with_carry(a: u64, b: u64, carry_in: u64, width_bits: u32) -> (u64, u64) {
    let mask: u128 = if width_bits == 64 {
        u128::from(u64::MAX)
    } else {
        (1u128 << width_bits) - 1
    };
    let sum = (u128::from(a) & mask) + (u128::from(b) & mask) + u128::from(carry_in);
    ((sum & mask) as u64, u64::from(sum > mask))
}

/// Runs the guest program starting at `entry`, until it returns to
/// [EXIT_MAGIC] or executes an exit syscall. Returns the exit status
/// (the value of x0 at the end).
pub fn run(
    cpu: &mut Cpu,
    backend: &mut dyn Backend,
    entry: u64,
    max_steps: Option<u64>,
) -> Result<u64, String> {
    cpu.pc = entry;
    let mut steps: u64 = 0;
    loop {
        if cpu.pc == EXIT_MAGIC {
            return Ok(cpu.x[0]);
        }
        if cpu.pc > u32::MAX as u64 {
            return Err(format!(
                "PC out of 32-bit guest address space: {:#x}",
                cpu.pc
            ));
        }
        if let Some(limit) = max_steps {
            if steps >= limit {
                return Err(format!(
                    "instruction limit ({limit}) reached at PC {:#x}",
                    cpu.pc
                ));
            }
        }
        steps += 1;

        let mut insn_bytes = [0u8; 4];
        if !backend.read(cpu.pc as u32, &mut insn_bytes) {
            return Err(format!("could not read instruction at PC {:#x}", cpu.pc));
        }
        let insn = u32::from_le_bytes(insn_bytes);
        let old_pc = cpu.pc;
        let mut next_pc = cpu.pc + 4;

        macro_rules! branch_to {
            ($target:expr) => {{
                let target: u64 = $target;
                if target > u32::MAX as u64 {
                    return Err(format!("branch target out of range: {:#x}", target));
                }
                next_pc = target;
            }};
        }

        let mut fault: Option<Option<String>> = None;
        let mut svc_result: Option<Result<SvcResult, String>> = None;

        // Wide immediate moves (MOVZ/MOVN/MOVK).
        if field(insn, 28, 23) == 0b100101 {
            let sf = field(insn, 31, 31);
            let opc = field(insn, 30, 29);
            let hw = field(insn, 22, 21);
            let imm16 = field(insn, 20, 5) as u64;
            let rd = field(insn, 4, 0);
            let shift = hw * 16;
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            match opc {
                0b00 => {
                    // MOVN
                    let value = !(imm16 << shift);
                    let value = if sf == 1 { value } else { value & 0xffff_ffff };
                    cpu.write_sp_or_reg(rd, value, width, sf == 1);
                }
                0b10 => {
                    // MOVZ
                    let value = imm16 << shift;
                    cpu.write_sp_or_reg(rd, value, width, sf == 1);
                }
                0b11 => {
                    // MOVK
                    let old = cpu.read_sp_or_reg(rd, width);
                    let mask = !(0xffffu64 << shift) & if sf == 1 { u64::MAX } else { 0xffff_ffff };
                    let value = (old & mask) | (imm16 << shift);
                    cpu.write_sp_or_reg(rd, value, width, sf == 1);
                }
                _ => return Err(unimplemented_insn(insn, old_pc)),
            }
        }
        // ADD/SUB (immediate).
        else if field(insn, 28, 23) == 0b100010 {
            let sf = field(insn, 31, 31);
            let op_sub = field(insn, 30, 30) == 1;
            let set_flags = field(insn, 29, 29) == 1;
            let shifted = field(insn, 22, 22) == 1;
            let imm12 = field(insn, 21, 10) as u64;
            let rn = field(insn, 9, 5);
            let rd = field(insn, 4, 0);
            let width_bits = if sf == 1 { 64 } else { 32 };
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            let a = cpu.read_sp_or_reg(rn, width);
            let b = if shifted { imm12 << 12 } else { imm12 };
            let (result, carry_out) = if op_sub {
                add_with_carry(a, !b, 1, width_bits)
            } else {
                add_with_carry(a, b, 0, width_bits)
            };
            if set_flags {
                cpu.set_flags(a, b, result, carry_out, op_sub, width_bits);
                cpu.write_sp_or_reg(rd, result, width, false);
            } else {
                cpu.write_sp_or_reg(rd, result, width, true);
            }
        }
        // ADD/SUB (shifted register) and comparisons.
        else if field(insn, 28, 24) == 0b01011 {
            if field(insn, 21, 21) == 1 {
                return Err(unimplemented_insn(insn, old_pc));
            }
            let sf = field(insn, 31, 31);
            let op_sub = field(insn, 30, 30) == 1;
            let set_flags = field(insn, 29, 29) == 1;
            let shift_kind = field(insn, 23, 22);
            let rm = field(insn, 20, 16);
            let imm6 = field(insn, 15, 10);
            let rn = field(insn, 9, 5);
            let rd = field(insn, 4, 0);
            let width_bits = if sf == 1 { 64 } else { 32 };
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            let a = if rn == 31 && !set_flags {
                cpu.sp
            } else {
                cpu.read_reg(rn, width)
            };
            let mut b = cpu.read_reg(rm, width);
            b = match shift_kind {
                0b00 => b << imm6,
                0b01 => b >> imm6,
                0b10 => ((sext(b, width_bits) as i64) >> imm6) as u64,
                _ => {
                    return Err(unimplemented_insn(insn, old_pc));
                }
            };
            let (result, carry_out) = if op_sub {
                add_with_carry(a, !b, 1, width_bits)
            } else {
                add_with_carry(a, b, 0, width_bits)
            };
            if set_flags {
                cpu.set_flags(a, b, result, carry_out, op_sub, width_bits);
                cpu.write_sp_or_reg(rd, result, width, false);
            } else {
                if rd == 31 {
                    cpu.sp = result;
                } else {
                    cpu.write_reg(rd, result, width);
                }
            }
        }
        // Logical (shifted register).
        else if field(insn, 28, 24) == 0b01010 {
            let sf = field(insn, 31, 31);
            let opc = field(insn, 30, 29);
            let set_flags = opc == 0b11;
            let shift_kind = field(insn, 23, 22);
            let invert = field(insn, 21, 21) == 1;
            let rm = field(insn, 20, 16);
            let imm6 = field(insn, 15, 10);
            let rn = field(insn, 9, 5);
            let rd = field(insn, 4, 0);
            let width_bits = if sf == 1 { 64 } else { 32 };
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            let a = cpu.read_reg(rn, width);
            let mut b = cpu.read_reg(rm, width);
            b = match shift_kind {
                0b00 => b << imm6,
                0b01 => b >> imm6,
                0b10 => ((sext(b, width_bits) as i64) >> imm6) as u64,
                _ => rotate_right64(b, imm6 as u64, width_bits),
            };
            if invert {
                b = !b & (u64::MAX >> (64 - width_bits));
            }
            let result: u64 = match opc {
                0b00 => a & b,
                0b01 => a | b,
                0b10 => a ^ b,
                _ => a & b,
            };
            if set_flags {
                let sign_bit = 1u64 << (width_bits - 1);
                cpu.n = (result & sign_bit) != 0;
                cpu.z = result == 0;
                cpu.c = false;
                cpu.v = false;
            }
            cpu.write_reg(rd, result, width);
        }
        // Logical (immediate).
        else if field(insn, 28, 23) == 0b100100 {
            let sf = field(insn, 31, 31);
            let opc = field(insn, 30, 29);
            let imm = decode_bitmask_imm(insn).ok_or_else(|| unimplemented_insn(insn, old_pc))?;
            let rn = field(insn, 9, 5);
            let rd = field(insn, 4, 0);
            let width_bits = if sf == 1 { 64 } else { 32 };
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            let a = cpu.read_reg(rn, width);
            let result: u64 = match opc {
                0b00 => a & imm,
                0b01 => a | imm,
                0b10 => a ^ imm,
                _ => a & imm,
            };
            if opc == 0b11 {
                let sign_bit = 1u64 << (width_bits - 1);
                cpu.n = (result & sign_bit) != 0;
                cpu.z = result == 0;
                cpu.c = false;
                cpu.v = false;
                cpu.write_sp_or_reg(rd, result, width, false);
            } else {
                cpu.write_reg(rd, result, width);
            }
        }
        // ADR/ADRP.
        else if field(insn, 28, 24) == 0b10000 {
            let is_adrp = field(insn, 31, 31) == 1;
            let immlo = field(insn, 30, 29);
            let immhi = field(insn, 23, 5);
            let rd = field(insn, 4, 0);
            let imm21 = ((immhi << 2) | immlo) as u64;
            let target = if is_adrp {
                let page = old_pc & !0xfff;
                page + sext(imm21, 21) * 4096
            } else {
                old_pc + sext(imm21, 21)
            };
            cpu.write_reg(rd, target, RegWidth::X);
        }
        // B (unconditional).
        else if field(insn, 31, 26) == 0b000101 {
            let imm26 = field(insn, 25, 0) as u64;
            branch_to!(old_pc + sext(imm26, 26) * 4);
        }
        // BL.
        else if field(insn, 31, 26) == 0b100101 {
            let imm26 = field(insn, 25, 0) as u64;
            cpu.write_reg(30, old_pc + 4, RegWidth::X);
            branch_to!(old_pc + sext(imm26, 26) * 4);
        }
        // B.cond.
        else if field(insn, 31, 24) == 0b01010100 {
            let imm19 = field(insn, 23, 5) as u64;
            let cond = field(insn, 3, 0);
            if cond_passes(cond, cpu)? {
                branch_to!(old_pc + sext(imm19, 19) * 4);
            }
        }
        // CBZ/CBNZ.
        else if field(insn, 30, 25) == 0b011010 {
            let sf = field(insn, 31, 31);
            let non_zero = field(insn, 24, 24) == 1;
            let imm19 = field(insn, 23, 5) as u64;
            let rt = field(insn, 4, 0);
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            let value = cpu.read_reg(rt, width);
            let taken = if non_zero { value != 0 } else { value == 0 };
            if taken {
                branch_to!(old_pc + sext(imm19, 19) * 4);
            }
        }
        // TBZ/TBNZ.
        else if field(insn, 30, 25) == 0b011011 {
            let b5 = field(insn, 31, 31);
            let b40 = field(insn, 23, 19);
            let bit = b5 * 32 + b40;
            let non_zero = field(insn, 24, 24) == 1;
            let imm14 = field(insn, 18, 5) as u64;
            let rt = field(insn, 4, 0);
            let value = cpu.read_reg(rt, RegWidth::X);
            let bit_set = (value >> bit) & 1 == 1;
            if bit_set == non_zero {
                branch_to!(old_pc + sext(imm14, 14) * 4);
            }
        }
        // BLR.
        else if insn & 0xffff_fc1f == 0xd63f_0000 {
            let rn = field(insn, 9, 5);
            let target = cpu.read_reg(rn, RegWidth::X);
            cpu.write_reg(30, old_pc + 4, RegWidth::X);
            branch_to!(target);
        }
        // BR / RET (these differ only in bit 22, which is ignored here).
        else if insn & 0xffbf_fc1f == 0xd61f_0000 {
            let rn = field(insn, 9, 5);
            branch_to!(cpu.read_reg(rn, RegWidth::X));
        }
        // SVC.
        else if insn & 0xffe0_001f == 0xd400_0001 {
            svc_result = Some(backend.syscall(cpu));
        }
        // NOP.
        else if insn == 0xd503_201f {
        }
        // Loads and stores.
        else if field(insn, 29, 27) == 0b111 && field(insn, 26, 26) == 0 {
            fault = Some(mem_op(insn, cpu, backend)?);
        }
        // STP/LDP.
        else if field(insn, 29, 27) == 0b101 && field(insn, 26, 26) == 0 {
            fault = Some(pair_op(insn, cpu, backend)?);
        }
        // Data processing (one source): UDIV/SDIV/LSLV/LSRV/ASRV/RORV.
        else if field(insn, 30, 29) == 0b00 && field(insn, 28, 21) == 0b11010110 {
            let sf = field(insn, 31, 31);
            let opcode = field(insn, 15, 10);
            let rm = field(insn, 20, 16);
            let rn = field(insn, 9, 5);
            let rd = field(insn, 4, 0);
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            let width_bits = if sf == 1 { 64 } else { 32 };
            let a = cpu.read_reg(rn, width);
            let b = cpu.read_reg(rm, width);
            match opcode {
                0b001000 => {
                    // LSLV
                    let amount = (b % width_bits as u64) as u32;
                    cpu.write_reg(rd, a << amount, width);
                }
                0b001001 => {
                    // LSRV
                    let amount = (b % width_bits as u64) as u32;
                    cpu.write_reg(rd, a >> amount, width);
                }
                0b001010 => {
                    // ASRV
                    let amount = (b % width_bits as u64) as u32;
                    cpu.write_reg(rd, ((sext(a, width_bits) as i64) >> amount) as u64, width);
                }
                0b001011 => {
                    // RORV
                    cpu.write_reg(
                        rd,
                        rotate_right64(a, b % width_bits as u64, width_bits),
                        width,
                    );
                }
                0b000010 => {
                    // UDIV (division by zero yields zero)
                    let result = a.checked_div(b).unwrap_or(0);
                    cpu.write_reg(rd, result, width);
                }
                0b000011 => {
                    // SDIV
                    let result = if b == 0 {
                        0
                    } else {
                        sext(a, width_bits).wrapping_div(sext(b, width_bits))
                    };
                    cpu.write_reg(rd, result, width);
                }
                _ => return Err(unimplemented_insn(insn, old_pc)),
            }
        }
        // MADD/MSUB/MUL/MNEG (data processing, three source).
        else if field(insn, 30, 24) == 0b0011011 {
            let sf = field(insn, 31, 31);
            let ra = field(insn, 14, 10);
            let rm = field(insn, 20, 16);
            let rn = field(insn, 9, 5);
            let rd = field(insn, 4, 0);
            let o1 = field(insn, 15, 15);
            let width = if sf == 1 { RegWidth::X } else { RegWidth::W };
            let a = cpu.read_reg(rn, width);
            let b = cpu.read_reg(rm, width);
            let c = cpu.read_reg(ra, width);
            let product = a.wrapping_mul(b);
            let result = if o1 == 0 {
                product.wrapping_add(c)
            } else {
                c.wrapping_sub(product)
            };
            cpu.write_reg(rd, result, width);
        }
        // Unimplemented.
        else {
            return Err(unimplemented_insn(insn, old_pc));
        }

        if let Some(result) = svc_result {
            match result? {
                SvcResult::Continue => {}
                SvcResult::Halt(status) => return Ok(status),
            }
        }
        if let Some(err) = fault.flatten() {
            return Err(err);
        }
        cpu.pc = next_pc;
    }
}

fn unimplemented_insn(insn: u32, pc: u64) -> String {
    format!("unimplemented ARM64 instruction {insn:#010x} at PC {pc:#x}")
}

/// Loads and stores (LDR/STR/LDRB/STRB/LDRH/STRH/LDRSW etc.), including the
/// unsigned-offset, unscaled, pre/post-index and register-offset variants.
/// Returns Some(error message) on a memory fault, None otherwise.
fn mem_op(insn: u32, cpu: &mut Cpu, backend: &mut dyn Backend) -> Result<Option<String>, String> {
    let size = field(insn, 31, 30);
    let opc = field(insn, 23, 22);
    let rn = field(insn, 9, 5);
    let rt = field(insn, 4, 0);
    let rn_bits = cpu.read_sp_or_reg(rn, RegWidth::X);

    let (addr, writeback) = if field(insn, 25, 24) == 0b01 {
        // Unsigned-offset variant.
        let imm12 = field(insn, 21, 10) as u64;
        let scale = 1u64 << size;
        (rn_bits.wrapping_add(imm12 * scale), None)
    } else if field(insn, 25, 24) == 0b00 {
        if field(insn, 21, 21) == 1 {
            // Register-offset variant.
            let rm = field(insn, 20, 16);
            let option = field(insn, 15, 13);
            let s = field(insn, 12, 12) == 1;
            if field(insn, 11, 10) != 0b10 {
                return Err(unimplemented_insn(insn, cpu.pc));
            }
            let rm_bits = cpu.read_reg(rm, RegWidth::X);
            let offset: u64 = match option {
                0b010 => rm_bits & 0xffff_ffff,
                0b011 => {
                    if s {
                        rm_bits << size
                    } else {
                        rm_bits
                    }
                }
                0b110 => sext(rm_bits & 0xffff_ffff, 32),
                0b111 => rm_bits,
                _ => return Err(unimplemented_insn(insn, cpu.pc)),
            };
            (rn_bits.wrapping_add(offset), None)
        } else {
            // Unscaled / pre/post-index variants (LDUR/STUR etc.).
            let imm9 = sign_extend32(field(insn, 20, 12), 9);
            let mode = field(insn, 11, 10);
            let mut addr = rn_bits;
            let mut wb: Option<(u32, i64)> = None;
            match mode {
                0b00 => {
                    addr = (rn_bits as i64 + imm9) as u64;
                }
                0b01 => {
                    wb = Some((rn, imm9));
                }
                0b10 => return Err(unimplemented_insn(insn, cpu.pc)),
                0b11 => {
                    addr = (rn_bits as i64 + imm9) as u64;
                    wb = Some((rn, imm9));
                }
                _ => unreachable!(),
            }
            (addr, wb)
        }
    } else {
        return Err(unimplemented_insn(insn, cpu.pc));
    };

    if addr > u32::MAX as u64 {
        return Err(format!("memory address out of range: {addr:#x}"));
    }
    let addr = addr as u32;

    let mut buf = [0u8; 8];
    let (len, is_load, is_signed) = match (size, opc) {
        (0b00, 0b00) => (1, false, false),
        (0b00, 0b01) => (1, true, false),
        (0b00, 0b10) => (1, true, true),
        (0b00, 0b11) => (1, true, true),
        (0b01, 0b00) => (2, false, false),
        (0b01, 0b01) => (2, true, false),
        (0b01, 0b10) => (2, true, true),
        (0b01, 0b11) => (2, true, true),
        (0b10, 0b00) => (4, false, false),
        (0b10, 0b01) => (4, true, false),
        (0b10, 0b10) => (4, true, true),
        (0b10, 0b11) => return Err(unimplemented_insn(insn, cpu.pc)),
        (0b11, 0b00) => (8, false, false),
        (0b11, 0b01) => (8, true, false),
        // PRFM is a hint; treat it as a no-op.
        (0b11, 0b10) => return Ok(None),
        (0b11, _) => return Err(unimplemented_insn(insn, cpu.pc)),
        _ => unreachable!(),
    };

    let mut error: Option<String> = None;
    if is_load {
        if backend.read(addr, &mut buf[..len]) {
            let raw = u64::from_le_bytes(buf);
            let value = if is_signed && len < 8 {
                sext(raw & ((1u64 << (len * 8)) - 1), (len * 8) as u32)
            } else if len < 8 {
                raw & ((1u64 << (len * 8)) - 1)
            } else {
                raw
            };
            // Signed loads with a Wt destination are sign-extended to 32
            // bits; writing the W register takes care of zero-extending.
            let width = if is_signed && opc == 0b11 {
                RegWidth::W
            } else {
                RegWidth::X
            };
            cpu.write_reg(rt, value, width);
        } else {
            error = Some(format!("memory read fault at {addr:#x}"));
        }
    } else {
        let value = cpu.read_reg(rt, RegWidth::X);
        buf[..len].copy_from_slice(&value.to_le_bytes()[..len]);
        if !backend.write(addr, &buf[..len]) {
            error = Some(format!("memory write fault at {addr:#x}"));
        }
    }

    if let Some((rn, imm)) = writeback {
        if rn == 31 {
            cpu.sp = (cpu.sp as i64 + imm) as u64;
        } else {
            cpu.x[rn as usize] = (cpu.x[rn as usize] as i64 + imm) as u64;
        }
    }

    Ok(error)
}

/// STP/LDP.
fn pair_op(insn: u32, cpu: &mut Cpu, backend: &mut dyn Backend) -> Result<Option<String>, String> {
    let opc = field(insn, 31, 30);
    let l = field(insn, 22, 22) == 1;
    let mode = field(insn, 24, 23);
    let imm7 = sign_extend32(field(insn, 21, 15), 7);
    let rt2 = field(insn, 14, 10);
    let rn = field(insn, 9, 5);
    let rt = field(insn, 4, 0);
    let scale: u64 = match opc {
        0b10 => 8,
        0b00 => 4,
        _ => return Err(unimplemented_insn(insn, cpu.pc)),
    };
    let offset = imm7 * scale as i64;
    let rn_bits = cpu.read_sp_or_reg(rn, RegWidth::X);

    let (addr, wb) = match mode {
        0b10 => (rn_bits as i64 + offset, None),
        0b01 => (rn_bits as i64, Some((rn, offset))),
        0b11 => (rn_bits as i64 + offset, Some((rn, offset))),
        _ => return Err(unimplemented_insn(insn, cpu.pc)),
    };
    if addr < 0 || addr > u32::MAX as i64 {
        return Err(format!("memory address out of range: {addr:#x}"));
    }
    let addr = addr as u32;

    let len = scale as usize;
    let mut buf = [0u8; 16];
    let mut error: Option<String> = None;
    if l {
        if backend.read(addr, &mut buf[..len * 2]) {
            let (a, b) = match len {
                1 => (buf[0] as u64, buf[1] as u64),
                2 => (
                    u16::from_le_bytes(buf[..2].try_into().unwrap()) as u64,
                    u16::from_le_bytes(buf[2..4].try_into().unwrap()) as u64,
                ),
                4 => (
                    u32::from_le_bytes(buf[..4].try_into().unwrap()) as u64,
                    u32::from_le_bytes(buf[4..8].try_into().unwrap()) as u64,
                ),
                8 => (
                    u64::from_le_bytes(buf[..8].try_into().unwrap()),
                    u64::from_le_bytes(buf[8..16].try_into().unwrap()),
                ),
                _ => unreachable!(),
            };
            cpu.write_reg(rt, a, RegWidth::X);
            cpu.write_reg(rt2, b, RegWidth::X);
        } else {
            error = Some(format!("memory read fault at {addr:#x}"));
        }
    } else {
        let mask = if len == 8 {
            u64::MAX
        } else {
            (1u64 << (len * 8)) - 1
        };
        let a = cpu.read_reg(rt, RegWidth::X) & mask;
        let b = cpu.read_reg(rt2, RegWidth::X) & mask;
        buf[..len].copy_from_slice(&a.to_le_bytes()[..len]);
        buf[len..len * 2].copy_from_slice(&b.to_le_bytes()[..len]);
        if !backend.write(addr, &buf[..len * 2]) {
            error = Some(format!("memory write fault at {addr:#x}"));
        }
    }

    if let Some((rn, imm)) = wb {
        if rn == 31 {
            cpu.sp = (cpu.sp as i64 + imm) as u64;
        } else {
            cpu.x[rn as usize] = (cpu.x[rn as usize] as i64 + imm) as u64;
        }
    }

    Ok(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMem {
        data: Vec<u8>,
    }

    impl TestMem {
        fn with_code(words: &[u32]) -> TestMem {
            let mut data = vec![0u8; 0x10000];
            for (i, w) in words.iter().enumerate() {
                data[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
            TestMem { data }
        }
    }

    impl Backend for TestMem {
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> bool {
            let addr = addr as usize;
            if addr + buf.len() > self.data.len() {
                return false;
            }
            buf.copy_from_slice(&self.data[addr..addr + buf.len()]);
            true
        }
        fn write(&mut self, addr: u32, buf: &[u8]) -> bool {
            let addr = addr as usize;
            if addr + buf.len() > self.data.len() {
                return false;
            }
            self.data[addr..addr + buf.len()].copy_from_slice(buf);
            true
        }
        fn syscall(&mut self, cpu: &mut Cpu) -> Result<SvcResult, String> {
            match cpu.x[16] {
                1 => Ok(SvcResult::Halt(cpu.x[0])),
                4 => {
                    let addr = cpu.x[1] as u32;
                    let len = cpu.x[2] as usize;
                    let mut s = vec![0u8; len];
                    assert!(self.read(addr, &mut s));
                    print!("{}", String::from_utf8_lossy(&s));
                    Ok(SvcResult::Continue)
                }
                n => Err(format!("unimplemented syscall {n}")),
            }
        }
    }

    const EXIT: u64 = EXIT_MAGIC;

    fn run_words(words: &[u32], entry: u64) -> (Result<u64, String>, Cpu) {
        let mut cpu = Cpu::new();
        cpu.x[30] = EXIT;
        cpu.sp = 0x8000;
        let mut mem = TestMem::with_code(words);
        let result = run(&mut cpu, &mut mem, entry, Some(1000));
        (result, cpu)
    }

    #[test]
    fn hello_test_fixture() {
        // mov w0, #0x12; add w0, w0, #1; ret
        let (result, _) = run_words(&[0x52800240, 0x11000400, 0xd65f03c0], 0);
        assert_eq!(result.unwrap(), 19);
    }

    #[test]
    fn wide_moves() {
        let (result, cpu) = run_words(
            &[
                0xd2800020, // movz x0, #1
                0xf2a00040, // movk x0, #2, lsl #16
                0x128000e5, // movn w5, #7
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[0], 0x20001);
        assert_eq!(cpu.x[5], 0xffff_fff8);
    }

    #[test]
    fn shifted_registers() {
        let (result, cpu) = run_words(
            &[
                0xd2800041, // movz x1, #2
                0xd2800062, // movz x2, #3
                0x8b050c83, // add x3, x4, x5, lsl #3 (all zero -> 0)
                0xd2800084, // movz x4, #4
                0xcb850883, // sub x3, x4, x5, asr #2 -> 4
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[3], 4);
        assert_eq!(cpu.x[1], 2);
        assert_eq!(cpu.x[2], 3);
    }
    #[test]
    fn compare_and_conditional_branch() {
        let (result, _) = run_words(
            &[
                0xd2800041, // movz x1, #2
                0xd2800062, // movz x2, #3
                0xeb02003f, // cmp x1, x2 -> not equal
                0x54000041, // b.ne +8 (skip the trap)
                0xd4200000, // trap (must be skipped)
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
    }
    #[test]
    fn adds_sets_z_flag() {
        let (result, _) = run_words(
            &[
                0xd2800041, // movz x1, #2
                0x51000422, // sub w2, w1, #1 -> 1
                0x71000421, // subs w1, w1, #1 -> 1
                0x7100103f, // subs wzr, w1, #4 -> cmp w1, #4 -> lt
                0x5400004b, // b.lt +8 (skip trap)
                0xd4200000, // trap (must be skipped)
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
    }
    #[test]
    fn memory_roundtrip() {
        let (result, cpu) = run_words(
            &[
                0xd2800080, // movz x0, #4
                0xf90007e0, // str x0, [sp, #8]
                0xd2800121, // movz x1, #9
                0xf94007e1, // ldr x1, [sp, #8]
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[1], 4);
    }
    #[test]
    fn byte_halfword_and_signed_loads() {
        let (result, cpu) = run_words(
            &[
                0x52801100, // movz w0, #0x88
                0x390003e0, // strb w0, [sp]
                0x398003e1, // ldrsb x1, [sp]
                0x52800162, // movz w2, #0xb (11)
                0x790007e2, // strh w2, [sp, #2]
                0x798007e3, // ldrsh x3, [sp, #2]
                0xb98003e4, // ldrsw x4, [sp]
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[1], 0xffff_ffff_ffff_ff88);
        assert_eq!(cpu.x[3], 11);
    }
    #[test]
    fn stp_ldp_pre_post() {
        let (result, cpu) = run_words(
            &[
                0xd2800080, // movz x0, #4
                0xd28000a1, // movz x1, #5
                0xa9bf7bfd, // stp x29, x30, [sp, #-16]!
                0xa8c17bfd, // ldp x29, x30, [sp], #16
                0xf90003e0, // str x0, [sp]
                0xf94003e2, // ldr x2, [sp]
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.sp, 0x8000);
        assert_eq!(cpu.x[2], 4);
        assert_eq!(cpu.x[30], EXIT);
    }
    #[test]
    fn stp_ldp_32bit() {
        let (result, cpu) = run_words(
            &[
                0xd2800080, // movz x0, #4
                0x29000fe2, // stp w2, w3, [sp]
                0x29400fe2, // ldp w2, w3, [sp]
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[2], 0);
        assert_eq!(cpu.x[3], 0);
        let _ = cpu;
    }
    #[test]
    fn register_offset_load_store() {
        let (result, cpu) = run_words(
            &[
                0xd2800080, // movz x0, #4
                0xf90007e0, // str x0, [sp, #8]
                0xd2800021, // movz x1, #1
                0xf8617be1, // ldr x1, [sp, x1, lsl #3] -> loads from sp+8
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[1], 4);
    }
    #[test]
    fn syscall_write_exit() {
        struct SysMem(TestMem);
        impl Backend for SysMem {
            fn read(&mut self, addr: u32, buf: &mut [u8]) -> bool {
                self.0.read(addr, buf)
            }
            fn write(&mut self, addr: u32, buf: &[u8]) -> bool {
                self.0.write(addr, buf)
            }
            fn syscall(&mut self, cpu: &mut Cpu) -> Result<SvcResult, String> {
                match cpu.x[16] {
                    4 => {
                        let mut s = vec![0u8; cpu.x[2] as usize];
                        assert!(self.0.read(cpu.x[1] as u32, &mut s));
                        assert_eq!(&s, b"hi");
                        Ok(SvcResult::Continue)
                    }
                    1 => Ok(SvcResult::Halt(cpu.x[0])),
                    n => Err(format!("unimplemented syscall {n}")),
                }
            }
        }
        let mut cpu = Cpu::new();
        cpu.x[30] = EXIT;
        cpu.sp = 0x8000;
        let mut mem = TestMem::with_code(&[
            0xd2800020, // movz x0, #1 (stdout)
            0x10000801, // adr x1, 0x100
            0xd2800042, // movz x2, #2
            0xd2800090, // movz x16, #4 (write)
            0xd4001001, // svc #0x80
            0xd28000e0, // movz x0, #7
            0xd2800030, // movz x16, #1 (exit)
            0xd4001001, // svc #0x80
            0xd65f03c0, // ret (unreachable)
        ]);
        // adr x1, #0x100 executes at PC 4, so the buffer address is 0x104.
        mem.data[0x104] = b'h';
        mem.data[0x105] = b'i';
        let result = run(&mut cpu, &mut SysMem(mem), 0, Some(100));
        assert_eq!(result.unwrap(), 7);
    }

    #[test]
    fn adrp_adr() {
        let adrp = 0x90000000 | ((0xf & 3) << 29) | ((0xf >> 2) << 5) | (1 << 0);
        let adr = 0x10000000 | ((0x10 & 3) << 29) | ((0x10 >> 2) << 5) | (2 << 0);
        let (result, cpu) = run_words(&[adrp, adr, 0xd65f03c0], 0);
        result.unwrap();
        // ADRP x1, #0xf000 (page-relative to its own PC 0)
        assert_eq!(cpu.x[1], 0xf000);
        // ADR x2, #0x10 executes at PC 4, so x2 = 4 + 0x10 = 0x14.
        assert_eq!(cpu.x[2], 0x14);
    }
    #[test]
    fn bl_and_return() {
        let mut cpu = Cpu::new();
        cpu.x[28] = EXIT;
        cpu.x[30] = EXIT;
        cpu.sp = 0x8000;
        let mut mem = TestMem::with_code(&[
            0x94000003, // 0x00: bl to 0xc (sets x30 = 0x4)
            0xaa1c03fe, // 0x04: mov x30, x28 - return lands here, restore link
            0xd65f03c0, // 0x08: ret - halt via x30 == EXIT_MAGIC
            0xd2800020, // 0x0c: movz x0, #1
            0xaa1e03e2, // 0x10: mov x2, x30 - capture return address (0x4)
            0xd65f03c0, // 0x14: ret - return to 0x4
        ]);
        let result = run(&mut cpu, &mut mem, 0, Some(1000));
        result.unwrap();
        assert_eq!(cpu.x[0], 1);
        assert_eq!(cpu.x[2], 4);
    }
    #[test]
    fn cbz_tbz() {
        let (result, _) = run_words(
            &[
                0xb4000045, // cbz x5, +8 (to the ret)
                0xd4200000, // trap (skipped because x5 == 0)
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
    }

    #[test]
    fn logical_immediates() {
        let (result, cpu) = run_words(
            &[
                0x92401ce6, // and x6, x7, #0xff (x7 = 0 -> 0)
                0xd2800107, // movz x7, #8
                0xb27c0ce6, // orr x6, x7, #0xf0 -> 0xf8
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[6], 0xf8);
    }

    #[test]
    fn mov_register() {
        let (result, cpu) = run_words(
            &[
                0xd2800041, // movz x1, #2
                0xaa0103e2, // mov x2, x1
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[2], 2);
    }
    #[test]
    fn udiv_mul() {
        let (result, cpu) = run_words(
            &[
                0xd28000e1, // movz x1, #7
                0xd2800042, // movz x2, #2
                0x9ac20820, // udiv x0, x1, x2 -> 3
                0x9b027c23, // mul x3, x1, x2 -> 14
                0xd65f03c0, // ret
            ],
            0,
        );
        result.unwrap();
        assert_eq!(cpu.x[0], 3);
        assert_eq!(cpu.x[3], 14);
    }
}

// ARM64 app loading and glue code
//
// The 32-bit emulation pipeline (Environment etc.) can't run 64-bit
// executables, so ARM64 apps get a much more minimal path: the executable is
// mapped into a fresh Mem, and _main is run by the interpreter above until it
// returns or exits.

use crate::bundle::Bundle;
use crate::fs::Fs;
use crate::mem::{Mem, Ptr};
use mach_object::{LoadCommand, MachCommand, OFile, ThreadState, CPU_TYPE_ARM64};
use std::io::Cursor;

/// Guest base address where ARM64 executables are mapped. ARM64 executables
/// are normally linked at 0x100000000, which doesn't fit in the 32-bit guest
/// address space, so they are slid down to this address instead.
const IMAGE_BASE: u32 = 0x5000_0000;

/// Address of the bottom byte (not the base) of the stack used for ARM64
/// apps. Nothing else uses this fresh Mem, so the exact address doesn't
/// matter, it just has to be far away from the mapped executable.
const STACK_TOP: u32 = 0x7ff0_0000;

/// Safety net so that a runaway ARM64 program can't loop forever.
const STEP_LIMIT: u64 = 10_000_000;

/// Returns true if `bytes` (the contents of an app's executable file) is a
/// 64-bit ARM Mach-O, i.e. something for the experimental ARM64 path.
pub fn detect_arm64_executable(bytes: &[u8]) -> bool {
    let mut cursor = Cursor::new(bytes);
    match OFile::parse(&mut cursor) {
        Ok(OFile::MachFile { header, .. }) => header.cputype == CPU_TYPE_ARM64,
        Ok(OFile::FatFile { files, .. }) => {
            // If there's a 32-bit ARM slice, prefer the established 32-bit
            // pipeline and only use the ARM64 path if ARM64 is all there is.
            let has_arm32 = files
                .iter()
                .any(|(arch, _)| arch.cputype == mach_object::CPU_TYPE_ARM);
            let has_arm64 = files.iter().any(|(arch, _)| arch.cputype == CPU_TYPE_ARM64);
            has_arm64 && !has_arm32
        }
        _ => false,
    }
}

struct Arm64Segment {
    segname: String,
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
}

struct Arm64Image {
    segments: Vec<Arm64Segment>,
    /// Entry point PC, as a (unslid) virtual address.
    entry_pc: Option<u64>,
    /// Entry point, as a file offset (from LC_MAIN).
    entry_off: Option<u64>,
    /// Initial stack pointer from LC_UNIXTHREAD, if any.
    entry_sp: Option<u64>,
}

fn parse_arm64_macho(bytes: &[u8]) -> Result<Arm64Image, String> {
    let mut cursor = Cursor::new(bytes);
    let file = OFile::parse(&mut cursor).map_err(|_| "Could not parse Mach-O file".to_string())?;
    let (header, commands) = match file {
        OFile::MachFile { header, commands } => (header, commands),
        OFile::FatFile { .. } => {
            return Err("Fat binaries are not supported by the ARM64 path yet".to_string())
        }
        _ => return Err("Unexpected Mach-O file kind: not an executable".to_string()),
    };
    if header.cputype != CPU_TYPE_ARM64 {
        return Err("Executable is not an ARM64 binary".to_string());
    }

    let mut image = Arm64Image {
        segments: Vec::new(),
        entry_pc: None,
        entry_off: None,
        entry_sp: None,
    };

    for MachCommand(command, _size) in commands {
        match command {
            LoadCommand::Segment64 {
                segname,
                vmaddr,
                vmsize,
                fileoff,
                filesize,
                ..
            } => {
                image.segments.push(Arm64Segment {
                    segname,
                    vmaddr: vmaddr as u64,
                    vmsize: vmsize as u64,
                    fileoff: fileoff as u64,
                    filesize: filesize as u64,
                });
            }
            LoadCommand::UnixThread {
                state: ThreadState::Arm64 { __pc, __sp, .. },
                ..
            } => {
                image.entry_pc = Some(__pc);
                image.entry_sp = Some(__sp);
            }
            LoadCommand::EntryPoint { entryoff, .. } => {
                image.entry_off = Some(entryoff);
            }
            _ => {}
        }
    }

    if image.entry_pc.is_none() && image.entry_off.is_none() {
        return Err(
            "Mach-O file does not specify an entry point, perhaps it is not an executable?"
                .to_string(),
        );
    }

    Ok(image)
}

/// Reads `buf` from guest memory, returning false if the access would go
/// outside the 32-bit guest address space.
fn guest_read(mem: &Mem, addr: u32, buf: &mut [u8]) -> bool {
    if addr as u64 + buf.len() as u64 > 0x1_0000_0000 {
        return false;
    }
    buf.copy_from_slice(
        mem.unchecked_bytes_at(Ptr::<u8, false>::from_bits(addr), buf.len() as u32),
    );
    true
}

/// Writes `buf` to guest memory, failing if the access would go outside the
/// 32-bit guest address space.
fn guest_write(mem: &mut Mem, addr: u32, buf: &[u8]) -> Result<(), String> {
    if addr as u64 + buf.len() as u64 > 0x1_0000_0000 {
        return Err(format!(
            "guest write outside 32-bit address space: {addr:#x}"
        ));
    }
    mem.bytes_at_mut(Ptr::<u8, true>::from_bits(addr), buf.len() as u32)
        .copy_from_slice(buf);
    Ok(())
}

struct MemBackend<'a> {
    mem: &'a mut Mem,
}

impl Backend for MemBackend<'_> {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> bool {
        guest_read(self.mem, addr, buf)
    }

    fn write(&mut self, addr: u32, buf: &[u8]) -> bool {
        guest_write(self.mem, addr, buf).is_ok()
    }

    fn syscall(&mut self, cpu: &mut Cpu) -> Result<SvcResult, String> {
        match cpu.x[16] {
            // exit
            1 => Ok(SvcResult::Halt(cpu.x[0])),
            // write(fd, buf, count): print to the host's stdout.
            4 => {
                let addr = cpu.x[1];
                let count = cpu.x[2];
                if count > 0x1000_0000 {
                    return Err(format!("unreasonably large write() of {count} bytes"));
                }
                if addr > u32::MAX as u64 || addr + count > 0x1_0000_0000 {
                    return Err(format!("write() from out-of-range address {addr:#x}"));
                }
                let mut bytes = vec![0u8; count as usize];
                if !guest_read(self.mem, addr as u32, &mut bytes) {
                    return Err(format!("write() from unmapped address {addr:#x}"));
                }
                echo!("{}", String::from_utf8_lossy(&bytes).trim_end_matches('\0'));
                Ok(SvcResult::Continue)
            }
            n => Err(format!("unimplemented ARM64 syscall {n}")),
        }
    }
}

/// Loads the ARM64 executable of `bundle` and runs its entry point in the
/// experimental ARM64 interpreter, returning the exit status.
pub fn run_arm64_app(bundle: &Bundle, fs: &Fs) -> Result<u64, String> {
    let bytes = fs
        .read(bundle.executable_path())
        .map_err(|_| "Could not read executable file".to_string())?;
    let image = parse_arm64_macho(&bytes)?;

    let text_base = image
        .segments
        .iter()
        .find(|s| s.segname == "__TEXT")
        .ok_or("Executable has no __TEXT segment")?
        .vmaddr;
    let slide = IMAGE_BASE as i64 - text_base as i64;

    let mut mem = Mem::new();
    for segment in &image.segments {
        if segment.segname == "__PAGEZERO" || segment.segname == "__LINKEDIT" {
            continue;
        }
        if segment.vmsize == 0 {
            continue;
        }
        let base = segment.vmaddr as i64 + slide;
        if base < 0 || base + segment.vmsize as i64 > u32::MAX as i64 {
            return Err(format!(
                "Segment {} does not fit in the 32-bit guest address space when slid to {:#x}",
                segment.segname, IMAGE_BASE
            ));
        }
        let base = base as u32;
        let vmsize = segment.vmsize as u32;
        log_dbg!(
            "Mapping {} at {:#x} (size {:#x})",
            segment.segname,
            base,
            vmsize
        );
        mem.reserve(base, vmsize);
        if segment.filesize > 0 {
            if segment.fileoff + segment.filesize > bytes.len() as u64 {
                return Err(format!(
                    "Segment {} extends beyond the end of the file",
                    segment.segname
                ));
            }
            let src = &bytes[segment.fileoff as usize..][..segment.filesize as usize];
            mem.bytes_at_mut(Ptr::from_bits(base), segment.filesize as u32)
                .copy_from_slice(src);
        }
    }

    let entry_pc = if let Some(pc) = image.entry_pc {
        pc
    } else {
        text_base + image.entry_off.unwrap()
    };
    let entry_pc = entry_pc as i64 + slide;
    if entry_pc < 0 || entry_pc > u32::MAX as i64 {
        return Err(format!(
            "Entry point {entry_pc:#x} is outside the guest address space"
        ));
    }
    let entry_pc = entry_pc as u64;

    let mut sp: u64 = STACK_TOP as u64;
    // Set up a minimal argv: a pointer to the program name, then a NULL
    // terminator. argc and a pointer to this array go in x0 and x1.
    let program_name = b"program\0";
    sp -= 16;
    guest_write(&mut mem, sp as u32, program_name)?;
    let argv0 = sp;
    sp -= 16;
    guest_write(&mut mem, sp as u32, &argv0.to_le_bytes())?;
    guest_write(&mut mem, (sp + 8) as u32, &0u64.to_le_bytes())?;

    let mut cpu = Cpu::new();
    cpu.x[0] = 1; // argc
    cpu.x[1] = sp; // argv
    cpu.x[2] = 0; // envp
    cpu.x[30] = EXIT_MAGIC;
    cpu.sp = image.entry_sp.unwrap_or(sp);
    // Note: the guest's own initial sp (if any) would be an unslid address,
    // which is almost certainly useless, so we always use our own stack.
    cpu.sp = sp;

    log_dbg!("Starting ARM64 execution at PC {entry_pc:#x}");
    let mut backend = MemBackend { mem: &mut mem };
    run(&mut cpu, &mut backend, entry_pc, Some(STEP_LIMIT))
}
