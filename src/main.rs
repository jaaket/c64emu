#[macro_use] extern crate lazy_static;
extern crate regex;
extern crate rustyline;

use std::collections::HashSet;
use std::fs::File;
use std::io::prelude::*;

use regex::Regex;
use rustyline::error::ReadlineError;

mod vic_ii;
use vic_ii::VicII;

struct StatusRegister {
    negative_flag: bool,
    overflow_flag: bool,
    // unused: bool
    break_flag: bool,
    decimal_mode_flag: bool,
    interrupt_disable_flag: bool,
    zero_flag: bool,
    carry_flag: bool
}

struct State {
    program_counter: u16,
    stack_pointer: u8,
    status_register: StatusRegister,
    accumulator: u8,
    index_x: u8,
    index_y: u8
}

struct Machine {
    memory: [u8; 65536],
    io: [u8; 65536],
    char_rom: [u8; 4096],
    vic_bank_start: u16,
    state: State,
    wait_cycles: i8,
    vic: VicII
}

const RESET_VECTOR_ADDR: usize = 0xfffc;

fn same_page(a: u16, b: u16) -> bool {
    a & 0xFF00 == b & 0xFF00
}

#[derive(PartialEq)]
enum MemoryRegion {
    ROM,
    CHAR_ROM
}

enum Effect {
    WriteMem { addr: u16, value: u8 }
}

impl Machine {
    fn new() -> Machine {
        Machine {
            memory: [0; 65536],
            io: [0; 65536],
            char_rom: [0; 4096],
            vic_bank_start: 0xC000,
            state: State {
                program_counter: 0,
                stack_pointer: 0,
                status_register: StatusRegister {
                    negative_flag: false,
                    overflow_flag: false,
                    // unused: true,
                    break_flag: true,
                    decimal_mode_flag: false,
                    interrupt_disable_flag: false,
                    zero_flag: false,
                    carry_flag: false
                },
                accumulator: 0,
                index_x: 0,
                index_y: 0
            },
            wait_cycles: 0,
            vic: VicII::new()
        }
    }

    fn reset(self: &mut Machine) {
        self.state.program_counter = self.memory[RESET_VECTOR_ADDR] as u16 | ((self.memory[RESET_VECTOR_ADDR + 1] as u16) << 8);
    }

    fn load_file(self: &mut Machine, filename: &str, memory_region: MemoryRegion, offset: usize) {
        {
            let f = File::open(filename).expect(&format!("file not found: {}", filename));
            let target =
                match memory_region {
                    MemoryRegion::ROM => &mut self.memory[offset..],
                    MemoryRegion::CHAR_ROM => &mut self.char_rom[offset..]
                };
            f.bytes().zip(target).for_each(|(byte, memory_byte)| *memory_byte = byte.unwrap());
        }

        if memory_region == MemoryRegion::CHAR_ROM {
            self.vic.char_rom.copy_from_slice(&self.char_rom[..]);
        }
    }

    fn stack(self: &mut Machine) -> &mut u8 {
        &mut self.memory[0x100 as usize + self.state.stack_pointer as usize]
    }

    fn push16(self: &mut Machine, value: u16) {
        *self.stack() = ((value & 0xFF00) >> 8) as u8;
        self.state.stack_pointer -= 1;
        *self.stack() = (value & 0x00FF) as u8;
        self.state.stack_pointer -= 1;
    }

    fn pop16(self: &mut Machine) -> u16 {
        self.state.stack_pointer += 1;
        let lo = *self.stack();
        self.state.stack_pointer += 1;
        let hi = *self.stack();
        ((hi as u16) << 8) + lo as u16
    }

    fn print_status(self: &Machine) {
        println!("pc      sp    n v - b d i z c  a     x     y     w");
        println!(
            "0x{:04X}  0x{:02X}  {} {} - {} {} {} {} {}  0x{:02X}  0x{:02X}  0x{:02X}  {}",
            self.state.program_counter,
            self.state.stack_pointer,
            if self.state.status_register.negative_flag { "1" } else { "0" },
            if self.state.status_register.overflow_flag { "1" } else { "0" },
            if self.state.status_register.break_flag { "1" } else { "0" },
            if self.state.status_register.decimal_mode_flag { "1" } else { "0" },
            if self.state.status_register.interrupt_disable_flag { "1" } else { "0" },
            if self.state.status_register.zero_flag { "1" } else { "0" },
            if self.state.status_register.carry_flag { "1" } else { "0" },
            self.state.accumulator,
            self.state.index_x,
            self.state.index_y,
            self.wait_cycles
        );
        println!(
            "0x{:02X} 0x{:02X} 0x{:02X}",
            self.memory[self.state.program_counter as usize],
            self.memory[self.state.program_counter as usize + 1],
            self.memory[self.state.program_counter as usize + 2]
        );
    }

    fn read_absolute_addr(self: &Machine) -> u16 {
        self.memory[self.state.program_counter as usize + 1] as u16 +
        ((self.memory[self.state.program_counter as usize + 2] as u16) << 8)
    }

    fn read_relative_addr(self: &Machine) -> u16 {
        let base = self.state.program_counter as i32;
        // ... as i8 as i32 <- first interpret as signed 8-bit value, then sign-extend to 32 bits
        let offset = self.memory[self.state.program_counter as usize + 1] as i8 as i32;
        (base + offset) as u16
    }

    fn read_mem(self: &Machine, addr: u16) -> u8 {
        // TODO: implement bank switching
        if addr >= 0xD000 && addr < 0xD400 {
            let vic_bank_start = self.vic_bank_start;
            self.vic.read(addr, &self.memory[vic_bank_start as usize..])
        } else if addr >= 0xD400 && addr < 0xE000 {
            self.io[addr as usize]
        } else {
            self.memory[addr as usize]
        }
    }

    fn read_immediate(self: &Machine) -> u8 {
        self.read_mem(self.state.program_counter + 1)
    }

    fn read_zeropage_addr(self: &Machine) -> u16 {
        self.read_mem(self.state.program_counter + 1) as u16
    }

    fn read_indirect_y_indexed_addr(self: &Machine) -> (u16, u16) {
        let vector_addr = self.read_zeropage_addr();
        let vector_lo = self.read_mem(vector_addr);
        let vector_hi = self.read_mem(vector_addr + 1);
        let vector = ((vector_hi as u16) << 8) + vector_lo as u16;
        (vector_addr, vector + self.state.index_y as u16)
    }

    fn read_indexed_zeropage_x(self: &Machine) -> (u16, u16) {
        let base_addr = self.read_immediate();
        let addr = base_addr.wrapping_add(self.state.index_x);
        (base_addr as u16, addr as u16)
    }

    fn write_mem(self: &mut Machine, addr: u16, value: u8) {
        // TODO: implement bank switching
        if (addr >= 0xA000 && addr < 0xC000) || addr >= 0xE000 {
            println!("Tried to write 0x{:02X} to ROM at 0x{:04X}, ignoring", value, addr);
        } else if addr >= 0xD000 && addr < 0xD400 {
            self.vic.write(addr, value);
        } else if addr >= 0xD400 && addr < 0xE000 {
            self.io[addr as usize] = value;
            if addr == 0xDD00 {
                self.vic_bank_start = 16384 * (0b11 - (value as u16 & 0b11));
                if value & 1 > 0 {
                    self.vic.enable_char_rom();
                } else {
                    self.vic.disable_char_rom();
                }
            }
        } else {
            self.memory[addr as usize] = value;
        }
    }

    fn set_negative_flag(self: &mut Machine, value: u8) {
        self.state.status_register.negative_flag = value & (1 << 7) != 0;
    }

    fn set_zero_flag(self: &mut Machine, value: u8) {
        self.state.status_register.zero_flag = value == 0;
    }

    fn compare(self: &mut Machine, operand1: u8, operand2: u8) {
        self.state.status_register.carry_flag = operand1 >= operand2;
        let value = operand1.wrapping_sub(operand2);
        self.set_negative_flag(value);
        self.set_zero_flag(value);
    }

    fn add_with_carry(self: &mut Machine, operand: u8) {
        let accumulator = self.state.accumulator;
        let added = accumulator as u16 + operand as u16 + if self.state.status_register.carry_flag { 1 } else { 0 };
        let value = added as u8;
        self.state.accumulator = value as u8;
        self.state.status_register.carry_flag = added & 0x0100 > 0;
        self.set_negative_flag(value);
        self.set_zero_flag(value);
        self.state.status_register.overflow_flag = (accumulator as i8) >= 0 && (operand as i8) >= 0 && (value as i8) < 0;
    }

    fn run_instruction(self: &mut Machine) -> Result<(String, Option<Effect>), String> {
        let opcode = self.read_mem(self.state.program_counter);

         match opcode {
            0x09 => {
                let operand = self.read_immediate();
                let value = self.state.accumulator | operand;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("ORA #${:02X}", operand),
                    None
                ));
            }
            0x0D => {
                let addr = self.read_absolute_addr();
                let operand = self.read_mem(addr);
                let value = self.state.accumulator | operand;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("ORA ${:04X}", addr),
                    None
                ));
            }
            0x10 => {
                let addr = self.read_relative_addr() + 2;
                if self.state.status_register.negative_flag == false {
                    self.state.program_counter = addr;
                    self.wait_cycles = if same_page(self.state.program_counter, addr) { 3 } else { 4 };
                } else {
                    self.state.program_counter += 2;
                    self.wait_cycles = 2;
                }
                return Ok((
                    format!("BPL ${:04X}", addr),
                    None
                ));
            }
            0x18 => {
                self.state.status_register.carry_flag = false;
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("CLC"),
                    None
                ));
            }
            0x20 => {
                let pc = self.state.program_counter;
                self.push16(pc + 2);
                let addr = self.read_absolute_addr();
                self.state.program_counter = addr;
                self.wait_cycles = 6;
                return Ok((
                    format!("JSR ${:04X}", addr),
                    None
                ));
            }
            0x29 => {
                let operand = self.read_immediate();
                let value = self.state.accumulator & operand;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("AND #${:02X}", operand),
                    None
                ));
            }
            0x2A => {
                let shifted = (self.state.accumulator as u16) << 1;
                let value = shifted as u8 | if self.state.status_register.carry_flag { 1 } else { 0 };
                self.state.status_register.carry_flag = shifted & 0x0100 > 0;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("ROL A"),
                    None
                ));
            }
            0x30 => {
                let addr = self.read_relative_addr() + 2;
                if self.state.status_register.negative_flag {
                    self.state.program_counter = addr;
                    self.wait_cycles = if same_page(self.state.program_counter, addr) { 3 } else { 4 };
                } else {
                    self.state.program_counter += 2;
                    self.wait_cycles = 2;
                }
                return Ok((
                    format!("BMI ${:04X}", addr),
                    None
                ));
            }
            0x38 => {
                self.state.status_register.carry_flag = true;
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("SEC"),
                    None
                ));
            }
            0x4C => {
                let addr = self.read_absolute_addr();
                self.state.program_counter = addr;
                self.wait_cycles = 3;
                return Ok((
                    format!("JMP ${:04X}", addr),
                    None
                ));
            }
            0x58 => {
                self.state.status_register.interrupt_disable_flag = false;
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("CLI"),
                    None
                ))
            }
            0x60 => {
                self.state.program_counter = self.pop16() + 1;
                self.wait_cycles = 6;
                return Ok((
                    format!("RTS"),
                    None
                ));
            }
            0x65 => {
                let addr = self.read_zeropage_addr();
                let operand = self.read_mem(addr);
                self.add_with_carry(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("ADC ${:02X}", addr),
                    None
                ));
            }
            0x69 => {
                let operand = self.read_immediate();
                self.add_with_carry(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("ADC #${:02X}", operand),
                    None
                ));
            }
            0x6C => {
                let vector_addr = self.read_absolute_addr();
                let vector_lo = self.read_mem(vector_addr);
                let vector_hi = self.read_mem(vector_addr + 1);
                let addr = ((vector_hi as u16) << 8) + vector_lo as u16;
                self.state.program_counter = addr;
                self.wait_cycles = 5;
                return Ok((
                    format!("JMP ({:04X})", vector_addr),
                    None
                ));
            }
            0x78 => {
                self.state.status_register.interrupt_disable_flag = true;
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("SEI"),
                    None
                ));
            }
            0x8D => {
                let addr = self.read_absolute_addr();
                let value = self.state.accumulator;
                self.write_mem(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("STA ${:04X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x85 => {
                let addr = self.read_zeropage_addr();
                let value = self.state.accumulator;
                self.write_mem(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("STA ${:02X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x84 => {
                let addr = self.read_zeropage_addr();
                let value = self.state.index_y;
                self.write_mem(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("STY ${:02X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x86 => {
                let addr = self.read_zeropage_addr();
                let value = self.state.index_x;
                self.write_mem(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("STX ${:02X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x88 => {
                let value = self.state.index_y.wrapping_sub(1);
                self.state.index_y = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("DEY"),
                    None
                ));
            }
            0x8A => {
                let value = self.state.index_x;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("TXA"),
                    None
                ));
            }
            0x8C => {
                let addr = self.read_absolute_addr();
                let value = self.state.index_y;
                self.write_mem(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("STY ${:04X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x8E => {
                let addr = self.read_absolute_addr();
                let value = self.state.index_x;
                self.write_mem(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("STX ${:04X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x90 => {
                let addr = self.read_relative_addr() + 2;
                if self.state.status_register.carry_flag == false {
                    self.state.program_counter = addr;
                    self.wait_cycles = if same_page(self.state.program_counter, addr) { 3 } else { 4 };
                } else {
                    self.state.program_counter += 2;
                    self.wait_cycles = 2;
                }
                return Ok((
                    format!("BCC ${:04X}", addr),
                    None
                ));
            }
            0x91 => {
                let (vector_addr, addr) = self.read_indirect_y_indexed_addr();
                let value = self.state.accumulator;
                self.write_mem(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 6;
                return Ok((
                    format!("STA (${:02X}),Y", vector_addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x94 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x();
                let value = self.state.index_y;
                self.write_mem(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 4;
                return Ok((
                    format!("STY ${:02X},X", base_addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x95 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x();
                let value = self.state.accumulator;
                self.write_mem(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 4;
                return Ok((
                    format!("STA ${:02X},X", base_addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x98 => {
                let value = self.state.index_y;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("TYA"),
                    None
                ));
            }
            0x99 => {
                let abs_addr = self.read_absolute_addr();
                let addr = abs_addr + self.state.index_y as u16;
                let value = self.state.accumulator;
                self.write_mem(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 5;
                return Ok((
                    format!("STA ${:04X},Y", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x9A => {
                self.state.stack_pointer = self.state.index_x;
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("TXS"),
                    None
                ));
            }
            0x9D => {
                let abs_addr = self.read_absolute_addr();
                let addr = abs_addr + self.state.index_x as u16;
                let value = self.state.accumulator;
                self.write_mem(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 5;
                return Ok((
                    format!("STA ${:04X},X", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0xA5 => {
                let addr = self.read_zeropage_addr();
                let value = self.read_mem(addr);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("LDA ${:02X}", addr),
                    None
                ));
            }
            0xAA => {
                let value = self.state.accumulator;
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("TAX"),
                    None
                ));
            }
            0xA0 => {
                let value = self.read_immediate();
                self.state.index_y = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("LDY #${:02X}", value),
                    None
                ));
            }
            0xA2 => {
                let value = self.read_mem(self.state.program_counter + 1);
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("LDX #${:02X}", value),
                    None
                ));
            }
            0xA4 => {
                let addr = self.read_zeropage_addr();
                let value = self.read_mem(addr);
                self.state.index_y = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("LDY ${:02X}", addr),
                    None
                ));
            }
            0xA6 => {
                let addr = self.read_zeropage_addr();
                let value = self.read_mem(addr);
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("LDX ${:02X}", addr),
                    None
                ));
            }
            0xA8 => {
                let value = self.state.accumulator;
                self.state.index_y = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("TAY"),
                    None
                ));
            }
            0xA9 => {
                let value = self.read_mem(self.state.program_counter + 1);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("LDA #${:02X}", value),
                    None
                ));
            }
            0xAC => {
                let addr = self.read_absolute_addr();
                let value = self.read_mem(addr);
                self.state.index_y = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("LDY ${:04X}", addr),
                    None
                ));
            }
            0xAD => {
                let addr = self.read_absolute_addr();
                let value = self.read_mem(addr);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("LDA ${:04X}", addr),
                    None
                ));
            }
            0xAE => {
                let addr = self.read_absolute_addr();
                let value = self.read_mem(addr);
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("LDX ${:04X}", addr),
                    None
                ));
            }
            0xB0 => {
                let addr = self.read_relative_addr() + 2;
                if self.state.status_register.carry_flag {
                    self.state.program_counter = addr;
                    self.wait_cycles = if same_page(self.state.program_counter, addr) { 3 } else { 4 };
                } else {
                    self.state.program_counter += 2;
                    self.wait_cycles = 2;
                }
                return Ok((
                    format!("BCS ${:04X}", addr),
                    None
                ));
            }
            0xB1 => {
                let (vector_addr, addr) = self.read_indirect_y_indexed_addr();
                let value = self.read_mem(addr);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = if same_page(self.state.program_counter, addr) { 5 } else { 6 };
                return Ok((
                    format!("LDA (${:02X}),Y", vector_addr),
                    None
                ));
            }
            0xB4 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x();
                let value = self.read_mem(addr);
                self.state.index_y = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 4;
                return Ok((
                    format!("LDY ${:02X},X", base_addr),
                    None
                ))
            }
            0xB5 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x();
                let value = self.read_mem(addr);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 4;
                return Ok((
                    format!("LDA ${:02X},X", base_addr),
                    None
                ));
            }
            0xB9 => {
                let abs_addr = self.read_absolute_addr();
                let addr = abs_addr + self.state.index_y as u16;
                let value = self.read_mem(addr);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                self.wait_cycles = if same_page(self.state.program_counter, addr) { 4 } else { 5 };
                return Ok((
                    format!("LDA ${:04X},Y", abs_addr),
                    None
                ));
            }
            0xBD => {
                let abs_addr = self.read_absolute_addr();
                let addr = abs_addr + self.state.index_x as u16;
                let value = self.read_mem(addr);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                self.wait_cycles = if same_page(self.state.program_counter, addr) { 4 } else { 5 };
                return Ok((
                    format!("LDA ${:04X},X", abs_addr),
                    None
                ));
            }
            0xC4 => {
                let operand1 = self.state.index_y;
                let addr = self.read_zeropage_addr();
                let operand2 = self.read_mem(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("CPY ${:02X}", addr),
                    None
                ));
            }
            0xC5 => {
                let operand1 = self.state.accumulator;
                let addr = self.read_zeropage_addr();
                let operand2 = self.read_mem(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("CMP ${:02X}", addr),
                    None
                ))
            }
            0xC8 => {
                let value = self.state.index_y.wrapping_add(1);
                self.state.index_y = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("INY"),
                    None
                ));
            }
            0xC9 => {
                let operand1 = self.state.accumulator;
                let operand2 = self.read_immediate();
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("CMP #${:02X}", operand2),
                    None
                ));
            }
            0xCA => {
                let value = self.state.index_x.wrapping_sub(1);
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("DEX"),
                    None
                ));
            }
            0xD0 => {
                let addr = self.read_relative_addr() + 2;
                if self.state.status_register.zero_flag == false {
                    self.state.program_counter = addr;
                    self.wait_cycles = if same_page(self.state.program_counter, addr) { 3 } else { 4 };
                } else {
                    self.state.program_counter += 2;
                    self.wait_cycles = 2;
                }
                return Ok((
                    format!("BNE ${:04X}", addr),
                    None
                ));
            }
            0xD1 => {
                let (vector_addr, addr) = self.read_indirect_y_indexed_addr();
                let operand1 = self.state.accumulator;
                let operand2 = self.read_mem(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = if same_page(self.state.program_counter, addr) { 5 } else { 6 };
                return Ok((
                    format!("CMP (${:02X}),Y", vector_addr),
                    None
                ));
            }
            0xD8 => {
                self.state.status_register.decimal_mode_flag = false;
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("CLD"),
                    None
                ));
            }
            0xDD => {
                let abs_addr = self.read_absolute_addr();
                let addr = abs_addr + self.state.index_x as u16;
                let operand1 = self.state.accumulator;
                let operand2 = self.read_mem(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 3;
                self.wait_cycles = if same_page(self.state.program_counter, addr) { 4 } else { 5 };
                return Ok((
                    format!("CMP ${:04X},X", abs_addr),
                    None
                ));
            }
            0xE0 => {
                let operand1 = self.state.index_x;
                let operand2 = self.read_immediate();
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("CPX #${:02X}", operand2),
                    None
                ));
            }
            0xE6 => {
                let addr = self.read_zeropage_addr();
                let value = self.read_mem(addr) + 1;
                self.write_mem(addr, value);
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 5;
                return Ok((
                    format!("INC ${:02X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
             0xE8 => {
                let value = self.state.index_x.wrapping_add(1);
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("INX"),
                    None
                ));
            }
            0xF0 => {
                let addr = self.read_relative_addr() + 2;
                if self.state.status_register.zero_flag {
                    self.state.program_counter = addr;
                    self.wait_cycles = if same_page(self.state.program_counter, addr) { 3 } else { 4 };
                } else {
                    self.state.program_counter += 2;
                    self.wait_cycles = 2;
                }
                return Ok((
                    format!("BEQ ${:04X}", addr),
                    None
                ));
            }
            _ => {
                let msg = format!("UNKNOWN OPCODE: 0x{:02X}", opcode);
                return Err(msg);
            }
        }
    }

    fn tick(self: &mut Machine) -> Result<(Option<String>, Option<Effect>), String> {
        let vic_bank_start = self.vic_bank_start;
        self.vic.tick(&self.memory[vic_bank_start as usize..]);

        self.wait_cycles -= 1;
        if self.wait_cycles <= 0 {
            self.run_instruction().map(|(name, eff_opt)| (Some(name), eff_opt))
        } else {
            Ok((None, None))
        }
    }
}

enum DebuggerCommand {
    Step,
    AddBreakpoint { addr: u16 },
    AddWatchpoint { addr: u16 },
    Run { verbose: bool },
    Exit,
    Inspect { addr: u16 }
}

fn parse_debugger_command(input: &str) -> Option<DebuggerCommand> {
    lazy_static! {
        static ref RUN: Regex = Regex::new("r$").unwrap();
        static ref RUN_VERBOSE: Regex = Regex::new("r v").unwrap();
        static ref ADD_BREAKPOINT: Regex = Regex::new(r"b ([0-9a-fA-F]{1,4})").unwrap();
        static ref ADD_WATCHPOINT: Regex = Regex::new(r"w ([0-9a-fA-F]{1,4})").unwrap();
        static ref INSPECT: Regex = Regex::new(r"i ([0-9a-fA-F]{1,4})").unwrap();
    }

    if RUN.is_match(input) {
        Some(DebuggerCommand::Run { verbose: false })
    } else if RUN_VERBOSE.is_match(input) {
        Some(DebuggerCommand::Run { verbose: true })
    } else if input.is_empty() {
        Some(DebuggerCommand::Step)
    } else if let Some(captures) = ADD_BREAKPOINT.captures(input) {
        let addr_str = &captures[1];
        match u16::from_str_radix(addr_str, 16) {
            Ok(addr) => Some(DebuggerCommand::AddBreakpoint { addr }),
            Err(_) => None
        }
    } else if let Some(captures) = ADD_WATCHPOINT.captures(input) {
        let addr_str = &captures[1];
        match u16::from_str_radix(addr_str, 16) {
            Ok(addr) => Some(DebuggerCommand::AddWatchpoint { addr }),
            Err(_) => None
        }
    } else if let Some(captures) = INSPECT.captures(input) {
        let addr_str = &captures[1];
        match u16::from_str_radix(addr_str, 16) {
            Ok(addr) => Some(DebuggerCommand::Inspect { addr }),
            Err(_) => None
        }
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum DebuggerState {
    Pause,
    Step,
    Run { verbose: bool }
}

struct Debugger {
    state: DebuggerState,
    breakpoints: HashSet<u16>,
    watchpoints: HashSet<u16>
}

impl Debugger {
    fn new() -> Debugger {
        Debugger {
            state: DebuggerState::Pause,
            breakpoints: HashSet::new(),
            watchpoints: HashSet::new()
        }
    }
}

fn main() {
    let mut machine = Machine::new();
    let mut debugger = Debugger::new();

    machine.load_file("basic.rom", MemoryRegion::ROM, 0xA000);
    machine.load_file("kernal.rom", MemoryRegion::ROM, 0xE000);
    machine.load_file("char.rom", MemoryRegion::CHAR_ROM, 0);

    machine.reset();

    let mut rl = rustyline::Editor::<()>::new();
    let history_path = "history.txt";
    if let Err(err) = rl.load_history(history_path) {
        println!("History not loaded: {:?}", err);
    }

    loop {
         match debugger.state {
            DebuggerState::Pause => {
                println!();
                machine.print_status();
            }
            DebuggerState::Step => {
                println!();
                machine.print_status();
                match machine.tick() {
                    Ok((Some(name), _)) => {
                        println!("{}", name);
                    }
                    Err(msg) => {
                        println!("{}", msg);
                    }
                    _ => {}
                }
            }
            DebuggerState::Run { verbose } => {
                loop {
                    if verbose {
                        println!();
                        machine.print_status();
                    }
                    if debugger.breakpoints.contains(&machine.state.program_counter) {
                        debugger.state = DebuggerState::Pause;
                        println!("Breakpoint at 0x{:04X} reached", machine.state.program_counter);
                        break;
                    }

                    match machine.tick() {
                        Ok((name_opt, Some(Effect::WriteMem { addr, value }))) => {
                            if let Some(name) = name_opt {
                                if verbose {
                                    println!("{}", name);
                                }
                            }
                            if debugger.watchpoints.contains(&addr) {
                                debugger.state = DebuggerState::Pause;
                                println!("Write detected at watchpoint: 0x{:02X} -> 0x{:04X}", value, addr);
                                break;
                            }
                        }
                        Ok((Some(name), None)) => {
                            if verbose {
                                println!("{}", name);
                            }
                        }
                        Ok((None, None)) => {}
                        Err(msg) => {
                            println!("{}", msg);
                            break;
                        }
                    }
                }
            }
        }

        let cmd = loop {
            match rl.readline("> ") {
                Ok(input) => {
                    rl.add_history_entry(&input);
                    if let Some(cmd) = parse_debugger_command(input.trim()) {
                        break cmd;
                    } else {
                        println!("Unknown command: {}", input);
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    break DebuggerCommand::Exit;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break DebuggerCommand::Exit;
                }
            }

        };

        match cmd {
            DebuggerCommand::Run { verbose } => {
                debugger.state = DebuggerState::Run { verbose };
            }
            DebuggerCommand::Step => {
                debugger.state = DebuggerState::Step;
            }
            DebuggerCommand::AddBreakpoint { addr } => {
                println!("Added breakpoint at 0x{:04X}", addr);
                debugger.breakpoints.insert(addr);
                debugger.state = DebuggerState::Pause;
            }
            DebuggerCommand::AddWatchpoint { addr } => {
                println!("Added watchpoint at 0x{:04X}", addr);
                debugger.watchpoints.insert(addr);
                debugger.state = DebuggerState::Pause;
            }
            DebuggerCommand::Inspect { addr } => {
                println!("Memory at 0x{:04X}: 0x{:02X}", addr, machine.read_mem(addr));
                debugger.state = DebuggerState::Pause;
            }
            DebuggerCommand::Exit => {
                break;
            }
        }
    }

    rl.save_history(history_path).unwrap();
}
