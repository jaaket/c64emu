use std::fs::File;
use std::io::prelude::*;

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
    state: State
}

const RESET_VECTOR_ADDR: usize = 0xfffc;

impl Machine {

    fn new() -> Machine {
        Machine {
            memory: [0; 65536],
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
            }
        }
    }

    fn reset(self: &mut Machine) {
        self.state.program_counter = self.memory[RESET_VECTOR_ADDR] as u16 | ((self.memory[RESET_VECTOR_ADDR + 1] as u16) << 8);
    }

    fn load_file(self: &mut Machine, filename: &str, offset: usize) {
        let f = File::open(filename).expect(&format!("file not found: {}", filename));
        f.bytes().zip(&mut self.memory[offset..]).for_each(|(byte, memory_byte)| *memory_byte = byte.unwrap());
    }

    fn stack(self: &mut Machine) -> &mut u8 {
        &mut self.memory[0x100 as usize + self.state.stack_pointer as usize]
    }

    fn push8(self: &mut Machine, value: u8) {
        *self.stack() = value;
        self.state.stack_pointer -= 1;
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
        println!("pc      sp    n v - b d i z c  a     x     y");
        println!(
            "0x{:04X}  0x{:02X}  {} {} - {} {} {} {} {}  0x{:02X}  0x{:02X}  0x{:02X}",
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
            self.state.index_y
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
        (self.state.program_counter as i32 + self.memory[self.state.program_counter as usize + 1] as i32) as u16
    }

    fn read_mem(self: &Machine, addr: u16) -> u8 {
        // TODO: implement bank switching
        self.memory[addr as usize]
    }

    fn write_mem(self: &mut Machine, addr: u16, value: u8) {
        // TODO: implement bank switching
        self.memory[addr as usize] = value;
    }

    fn set_negative_flag(self: &mut Machine, value: u8) {
        self.state.status_register.negative_flag = value & (1 << 7) != 0;
    }

    fn set_zero_flag(self: &mut Machine, value: u8) {
        self.state.status_register.zero_flag = value == 0;
    }

    fn run_instruction(self: &mut Machine) {
        let opcode = self.read_mem(self.state.program_counter);

         match opcode {
            0x20 => {
                let pc = self.state.program_counter;
                self.push16(pc);
                let addr = self.read_absolute_addr();
                self.state.program_counter = addr;
                println!("JSR ${:04X}", addr);
            },
            0x60 => {
                self.state.program_counter = self.pop16() + 3;
                println!("RTS");
            }
            0x78 => {
                self.state.status_register.interrupt_disable_flag = true;
                self.state.program_counter += 1;
                println!("SEI");
            },
            0x8D => {
                let addr = self.read_absolute_addr();
                let value = self.read_mem(addr);
                self.write_mem(addr, value);
                self.state.program_counter += 3;
                println!("STA ${:04X}", addr);
            },
            0x8E => {
                let addr = self.read_absolute_addr();
                let value = self.state.index_x;
                self.write_mem(addr, value);
                self.state.program_counter += 3;
                println!("STX ${:04X}", addr);
            },
            0x9A => {
                self.state.stack_pointer = self.state.index_x;
                self.state.program_counter += 1;
                println!("TXS");
            },
            0xA2 => {
                let value = self.read_mem(self.state.program_counter + 1);
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                println!("LDX #${:02X}", value);
            },
            0xA9 => {
                let value = self.read_mem(self.state.program_counter + 1);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                println!("LDA #${:02X}", value);
            },
            0xCA => {
                let value = self.state.index_x.wrapping_sub(1);
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                println!("DEX");
            }
            0xD0 => {
                let addr = self.read_relative_addr() + 2;
                if self.state.status_register.zero_flag == false {
                    self.state.program_counter = addr;
                } else {
                    self.state.program_counter += 2;
                }
                println!("BNE ${:04X}", addr);
            },
            0xBD => {
                let abs_addr = self.read_absolute_addr();
                let addr = abs_addr + self.state.index_x as u16;
                let value = self.read_mem(addr);
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                println!("LDA ${:04X},X", abs_addr);
            },
            0xD8 => {
                self.state.status_register.decimal_mode_flag = false;
                self.state.program_counter += 1;
                println!("CLD");
            },
            0xDD => {
                let abs_addr = self.read_absolute_addr();
                let addr = abs_addr + self.state.index_x as u16;
                let operand = self.read_mem(addr);
                self.state.status_register.carry_flag = self.state.accumulator >= operand;
                let value = self.state.accumulator.wrapping_sub(operand);
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 3;
                println!("CMP ${:04X},X", abs_addr);
            },
            _ => {
                println!("UNKNOWN OPCODE: 0x{:02X}", opcode);
            }
        }
    }
}

fn main() {
    let mut machine = Machine::new();

    machine.load_file("basic.rom", 0xA000);
    machine.load_file("kernal.rom", 0xE000);

    machine.reset();

    let mut input = String::new();

    loop {
        machine.print_status();
        machine.run_instruction();
        std::io::stdin().read_line(&mut input);
    }
}
