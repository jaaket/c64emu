use memory::{ReadView, WriteView};

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

pub struct Mos6510 {
    state: State,
    wait_cycles: i8,
    irq: bool
}

const RESET_VECTOR_ADDR: u16 = 0xfffc;

fn same_page(a: u16, b: u16) -> bool {
    a & 0xFF00 == b & 0xFF00
}

pub enum Effect {
    WriteMem { addr: u16, value: u8 }
}

impl Mos6510 {
    pub fn new() -> Mos6510 {
        Mos6510 {
            state: State {
                program_counter: 0,
                stack_pointer: 0,
                status_register: StatusRegister {
                    negative_flag: false,
                    overflow_flag: false,
                    // unused: true,
                    break_flag: false,
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
            irq: false
         }
    }

    pub fn print_status(self: &Mos6510) {
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
    }

    pub fn reset<M: ReadView>(self: &mut Mos6510, mem: &mut M) {
        self.state.program_counter = mem.read(RESET_VECTOR_ADDR) as u16 | ((mem.read(RESET_VECTOR_ADDR + 1) as u16) << 8);
    }

    pub fn get_pc(self: &Mos6510) -> u16 {
        self.state.program_counter
    }

    fn effective_stack_pointer(self: &Mos6510) -> u16 {
        0x100 + self.state.stack_pointer as u16
    }

    fn push8<M: WriteView>(self: &mut Mos6510, mem: &mut M, value: u8) {
        mem.write(self.effective_stack_pointer(), value);
        self.state.stack_pointer -= 1;
    }

    fn push16<M: WriteView>(self: &mut Mos6510, mem: &mut M, value: u16) {
        mem.write(self.effective_stack_pointer(), ((value & 0xFF00) >> 8) as u8);
        self.state.stack_pointer -= 1;
        mem.write(self.effective_stack_pointer(), (value & 0x00FF) as u8);
        self.state.stack_pointer -= 1;
    }

    fn pop8<M: ReadView>(self: &mut Mos6510, mem: &mut M) -> u8 {
        self.state.stack_pointer += 1;
        mem.read(self.effective_stack_pointer())
    }

    fn pop16<M: ReadView>(self: &mut Mos6510, mem: &mut M) -> u16 {
        self.state.stack_pointer += 1;
        let lo = mem.read(self.effective_stack_pointer());
        self.state.stack_pointer += 1;
        let hi = mem.read(self.effective_stack_pointer());
        ((hi as u16) << 8) + lo as u16
    }

    fn read_absolute_addr<M: ReadView>(self: &Mos6510, mem: &mut M) -> u16 {
        mem.read(self.state.program_counter + 1) as u16 +
        ((mem.read(self.state.program_counter + 2) as u16) << 8)
    }

    fn read_relative_addr<M: ReadView>(self: &Mos6510, mem: &mut M) -> u16 {
        let base = self.state.program_counter as i32;
        // ... as i8 as i32 <- first interpret as signed 8-bit value, then sign-extend to 32 bits
        let offset = mem.read(self.state.program_counter + 1) as i8 as i32;
        (base + offset) as u16
    }

    fn read_immediate<M: ReadView>(self: &Mos6510, mem: &mut M) -> u8 {
        mem.read(self.state.program_counter + 1)
    }

    fn read_zeropage_addr<M: ReadView>(self: &Mos6510, mem: &mut M) -> u16 {
        mem.read(self.state.program_counter + 1) as u16
    }

    fn read_indirect_y_indexed_addr<M: ReadView>(self: &Mos6510, mem: &mut M) -> (u16, u16) {
        let vector_addr = self.read_zeropage_addr(mem);
        let vector_lo = mem.read(vector_addr);
        let vector_hi = mem.read(vector_addr + 1);
        let vector = ((vector_hi as u16) << 8) + vector_lo as u16;
        (vector_addr, vector + self.state.index_y as u16)
    }

    fn read_indexed_zeropage_x<M: ReadView>(self: &Mos6510, mem: &mut M) -> (u16, u16) {
        let base_addr = self.read_immediate(mem);
        let addr = base_addr.wrapping_add(self.state.index_x);
        (base_addr as u16, addr as u16)
    }

    fn set_negative_flag(self: &mut Mos6510, value: u8) {
        self.state.status_register.negative_flag = value & (1 << 7) != 0;
    }

    fn set_zero_flag(self: &mut Mos6510, value: u8) {
        self.state.status_register.zero_flag = value == 0;
    }

    fn compare(self: &mut Mos6510, operand1: u8, operand2: u8) {
        self.state.status_register.carry_flag = operand1 >= operand2;
        let value = operand1.wrapping_sub(operand2);
        self.set_negative_flag(value);
        self.set_zero_flag(value);
    }

    fn add_with_carry(self: &mut Mos6510, operand: u8) {
        let accumulator = self.state.accumulator;
        let added = accumulator as u16 + operand as u16 + if self.state.status_register.carry_flag { 1 } else { 0 };
        let value = added as u8;
        self.state.accumulator = value as u8;
        self.state.status_register.carry_flag = added & 0x0100 > 0;
        self.set_negative_flag(value);
        self.set_zero_flag(value);
        self.state.status_register.overflow_flag = (accumulator as i8) >= 0 && (operand as i8) >= 0 && (value as i8) < 0;
    }

    fn subtract_with_carry(self: &mut Mos6510, operand: u8) {
        let accumulator = self.state.accumulator;
        let subtracted = accumulator as i8 as i16 - operand as i8 as i16 - if self.state.status_register.carry_flag { 0 } else { 1 };
        let value = subtracted as u8;
        self.state.accumulator = value;
        self.state.status_register.carry_flag = (accumulator as u8) >= (operand as u8);
        self.set_negative_flag(value);
        self.set_zero_flag(value);
        self.state.status_register.overflow_flag = subtracted < -128 || subtracted > 127;
    }

    fn shift_left_memory<M: ReadView + WriteView>(self: &mut Mos6510, mem: &mut M, addr: u16) -> Effect {
        let operand = mem.read(addr);
        let shifted = (operand as u16) << 1;
        let value = shifted as u8;
        mem.write(addr, value);
        self.state.status_register.carry_flag = shifted & 0x0100 > 0;
        self.set_negative_flag(value);
        self.set_zero_flag(value);
        Effect::WriteMem { addr, value }
    }

    fn rotate_right_memory<M: ReadView + WriteView>(self: &mut Mos6510, mem: &mut M, addr: u16) -> Effect {
        let operand = mem.read(addr);
        let value = ((if self.state.status_register.carry_flag { 0x100 } else { 0 } | operand as u16) >> 1) as u8;
        mem.write(addr, value);
        self.state.status_register.carry_flag = operand & 1 > 0;
        self.set_negative_flag(value);
        self.set_zero_flag(value);
        Effect::WriteMem { addr, value }
    }

    fn decrement_memory<M: ReadView + WriteView>(self: &mut Mos6510, mem: &mut M, addr: u16) -> Effect {
        let operand = mem.read(addr);
        let value = operand.wrapping_sub(1);
        mem.write(addr, value);
        self.set_negative_flag(value);
        self.set_zero_flag(value);
        Effect::WriteMem { addr, value }
    }

    fn or_with_accumulator(self: &mut Mos6510, operand: u8) {
        let value = self.state.accumulator | operand;
        self.state.accumulator = value;
        self.set_negative_flag(value);
        self.set_zero_flag(value);
    }

    fn status_register_value(self: &Mos6510) -> u8 {
        let value =
            if self.state.status_register.carry_flag             { 0b0000_0001 } else { 0 } |
            if self.state.status_register.zero_flag              { 0b0000_0010 } else { 0 } |
            if self.state.status_register.interrupt_disable_flag { 0b0000_0100 } else { 0 } |
            if self.state.status_register.decimal_mode_flag      { 0b0000_1000 } else { 0 } |
            if self.state.status_register.break_flag             { 0b0001_0000 } else { 0 } |
            if self.state.status_register.overflow_flag          { 0b0100_0000 } else { 0 } |
            if self.state.status_register.negative_flag          { 0b1000_0000 } else { 0 };
        value
    }

    fn set_status_register(self: &mut Mos6510, value: u8) {
        self.state.status_register.carry_flag             = value & 0b0000_0001 > 0;
        self.state.status_register.zero_flag              = value & 0b0000_0010 > 0;
        self.state.status_register.interrupt_disable_flag = value & 0b0000_0100 > 0;
        self.state.status_register.decimal_mode_flag      = value & 0b0000_1000 > 0;
        self.state.status_register.break_flag             = value & 0b0001_0000 > 0;
        self.state.status_register.overflow_flag          = value & 0b0100_0000 > 0;
        self.state.status_register.negative_flag          = value & 0b1000_0000 > 0;
    }

    pub fn tick<M: ReadView + WriteView>(self: &mut Mos6510, mem: &mut M, irq: bool) -> Result<(Option<String>, Option<Effect>), String> {
        if irq {
            self.irq = true;
        }
        self.wait_cycles -= 1;
        if self.wait_cycles <= 0 {
            if self.irq && !self.state.status_register.interrupt_disable_flag {
                let pc = self.state.program_counter;
                let sr = self.status_register_value();
                self.push16(mem, pc);
                self.push8(mem, sr);
                self.state.program_counter = 0xFF48;
                self.irq = false;
                Ok((None, None))
            } else {
                self.run_instruction(mem).map(|(name, eff_opt)| (Some(name), eff_opt))
            }
        } else {
            Ok((None, None))
        }
    }

    pub fn run_instruction<M: ReadView + WriteView>(self: &mut Mos6510, mem: &mut M) -> Result<(String, Option<Effect>), String> {
        let opcode = mem.read(self.state.program_counter);

        match opcode {
            0x05 => {
                let addr = self.read_zeropage_addr(mem);
                let operand = mem.read(addr);
                self.or_with_accumulator(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("ORA ${:02X}", addr),
                    None
                ));
            }
            0x06 => {
                let addr = self.read_zeropage_addr(mem);
                let effect = self.shift_left_memory(mem, addr);
                self.state.program_counter += 2;
                self.wait_cycles = 5;
                return Ok((
                    format!("ASL ${:02X}", addr),
                    Some(effect)
                ))
            }
            0x08 => {
                let value = self.status_register_value();
                self.push8(mem, value);
                self.state.program_counter += 1;
                self.wait_cycles = 3;
                return Ok((
                    format!("PHP"),
                    None
                ));
            }
            0x09 => {
                let operand = self.read_immediate(mem);
                self.or_with_accumulator(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("ORA #${:02X}", operand),
                    None
                ));
            }
            0x0A => {
                let operand = self.state.accumulator;
                let value = operand << 1;
                self.state.status_register.carry_flag = operand & 0x80 > 0;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("ASL"),
                    None
                ));
            }
            0x0D => {
                let addr = self.read_absolute_addr(mem);
                let operand = mem.read(addr);
                self.or_with_accumulator(operand);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("ORA ${:04X}", addr),
                    None
                ));
            }
            0x10 => {
                let addr = self.read_relative_addr(mem) + 2;
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
            0x16 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x(mem);
                let effect = self.shift_left_memory(mem, addr);
                self.state.program_counter += 2;
                self.wait_cycles = 6;
                return Ok((
                    format!("ASL ${:02X},X", base_addr),
                    Some(effect)
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
                self.push16(mem, pc + 2);
                let addr = self.read_absolute_addr(mem);
                self.state.program_counter = addr;
                self.wait_cycles = 6;
                return Ok((
                    format!("JSR ${:04X}", addr),
                    None
                ));
            }
            0x24 => {
                let addr = self.read_zeropage_addr(mem);
                let operand = mem.read(addr);
                let value = self.state.accumulator & operand;
                self.set_zero_flag(value);
                self.state.status_register.negative_flag = operand & 0b1000_0000 > 0;
                self.state.status_register.overflow_flag = operand & 0b0100_0000 > 0;
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("BIT ${:02X}", addr),
                    None
                ))
            }
            0x28 => {
                let value = self.pop8(mem);
                self.set_status_register(value);
                self.state.program_counter += 1;
                self.wait_cycles = 4;
                return Ok((
                    format!("PLP"),
                    None
                ));
            }
            0x29 => {
                let operand = self.read_immediate(mem);
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
            0x2C => {
                let addr = self.read_absolute_addr(mem);
                let operand = mem.read(addr);
                let value = self.state.accumulator & operand;
                self.set_zero_flag(value);
                self.state.status_register.negative_flag = operand & 0b1000_0000 > 0;
                self.state.status_register.overflow_flag = operand & 0b0100_0000 > 0;
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("BIT ${:04X}", addr),
                    None
                ));
            }
            0x30 => {
                let addr = self.read_relative_addr(mem) + 2;
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
            0x40 => {
                let sr = self.pop8(mem);
                let pc = self.pop16(mem);
                self.set_status_register(sr);
                self.state.program_counter = pc;
                return Ok((
                    format!("RTI"),
                    None
                ));
            }
            0x45 => {
                let addr = self.read_zeropage_addr(mem);
                let operand = mem.read(addr);
                let value = self.state.accumulator ^ operand;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("EOR ${:02X}", addr),
                    None
                ));
            }
            0x46 => {
                let addr = self.read_zeropage_addr(mem);
                let operand = mem.read(addr);
                let value = operand >> 1;
                mem.write(addr, value);
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.status_register.carry_flag = operand & 1 > 0;
                self.state.program_counter += 2;
                self.wait_cycles = 5;
                return Ok((
                    format!("LSR ${:02X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x48 => {
                let value = self.state.accumulator;
                self.push8(mem, value);
                self.state.program_counter += 1;
                self.wait_cycles = 3;
                return Ok((
                    format!("PHA"),
                    None
                ));
            }
            0x49 => {
                let operand = self.read_immediate(mem);
                let value = self.state.accumulator ^ operand;
                self.state.accumulator = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("EOR #${:02X}", operand),
                    None
                ));
            }
            0x4A => {
                let operand = self.state.accumulator;
                let value = operand >> 1;
                self.state.status_register.carry_flag = operand & 1 > 0;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("LSR"),
                    None
                ));
            }
            0x4C => {
                let addr = self.read_absolute_addr(mem);
                self.state.program_counter = addr;
                self.wait_cycles = 3;
                return Ok((
                    format!("JMP ${:04X}", addr),
                    None
                ));
            }
            0x56 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x(mem);
                let operand = mem.read(addr);
                let value = operand >> 1;
                mem.write(addr, value);
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.status_register.carry_flag = operand & 1 > 0;
                self.state.program_counter += 2;
                self.wait_cycles = 5;
                return Ok((
                    format!("LSR ${:02X},X", base_addr),
                    Some(Effect::WriteMem { addr, value })
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
                self.state.program_counter = self.pop16(mem) + 1;
                self.wait_cycles = 6;
                return Ok((
                    format!("RTS"),
                    None
                ));
            }
            0x65 => {
                let addr = self.read_zeropage_addr(mem);
                let operand = mem.read(addr);
                self.add_with_carry(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("ADC ${:02X}", addr),
                    None
                ));
            }
            0x66 => {
                let addr = self.read_zeropage_addr(mem);
                let effect = self.rotate_right_memory(mem, addr);
                self.state.program_counter += 2;
                self.wait_cycles = 5;
                return Ok((
                    format!("ROR ${:02X}", addr),
                    Some(effect)
                ));
            }
            0x68 => {
                self.state.accumulator = self.pop8(mem);
                self.state.program_counter += 1;
                self.wait_cycles = 4;
                return Ok((
                    format!("PLA"),
                    None
                ));
            }
            0x69 => {
                let operand = self.read_immediate(mem);
                self.add_with_carry(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("ADC #${:02X}", operand),
                    None
                ));
            }
            0x6A => {
                let operand = self.state.accumulator;
                let value = ((if self.state.status_register.carry_flag { 0x100 } else { 0 } | operand as u16) >> 1) as u8;
                self.state.accumulator = value;
                self.state.status_register.carry_flag = operand & 1 > 0;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("ROR A"),
                    None
                ));
            }
            0x6C => {
                let vector_addr = self.read_absolute_addr(mem);
                let vector_lo = mem.read(vector_addr);
                let vector_hi = mem.read(vector_addr + 1);
                let addr = ((vector_hi as u16) << 8) + vector_lo as u16;
                self.state.program_counter = addr;
                self.wait_cycles = 5;
                return Ok((
                    format!("JMP ({:04X})", vector_addr),
                    None
                ));
            }
            0x70 => {
                let addr = self.read_relative_addr(mem) + 2;
                if self.state.status_register.overflow_flag {
                    self.state.program_counter = addr;
                    self.wait_cycles = if same_page(self.state.program_counter, addr) { 3 } else { 4 };
                } else {
                    self.state.program_counter += 2;
                    self.wait_cycles = 2;
                }
                return Ok((
                    format!("BVS ${:04X}", addr),
                    None
                ));
            }
            0x76 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x(mem);
                let effect = self.rotate_right_memory(mem, addr);
                self.state.program_counter += 2;
                self.wait_cycles = 6;
                return Ok((
                    format!("ROR ${:02X},X", base_addr),
                    Some(effect)
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
            0x79 => {
                let abs_addr = self.read_absolute_addr(mem);
                let addr = abs_addr + self.state.index_y as u16;
                let operand = mem.read(addr);
                self.add_with_carry(operand);
                self.state.program_counter += 3;
                self.wait_cycles = if same_page(self.state.program_counter, addr) { 4 } else { 5 };
                return Ok((
                    format!("ADC ${:04X},Y", abs_addr),
                    None
                ));
            }
            0x8D => {
                let addr = self.read_absolute_addr(mem);
                let value = self.state.accumulator;
                mem.write(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("STA ${:04X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x85 => {
                let addr = self.read_zeropage_addr(mem);
                let value = self.state.accumulator;
                mem.write(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("STA ${:02X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x84 => {
                let addr = self.read_zeropage_addr(mem);
                let value = self.state.index_y;
                mem.write(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("STY ${:02X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x86 => {
                let addr = self.read_zeropage_addr(mem);
                let value = self.state.index_x;
                mem.write(addr, value);
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
                let addr = self.read_absolute_addr(mem);
                let value = self.state.index_y;
                mem.write(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("STY ${:04X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x8E => {
                let addr = self.read_absolute_addr(mem);
                let value = self.state.index_x;
                mem.write(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("STX ${:04X}", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x90 => {
                let addr = self.read_relative_addr(mem) + 2;
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
                let (vector_addr, addr) = self.read_indirect_y_indexed_addr(mem);
                let value = self.state.accumulator;
                mem.write(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 6;
                return Ok((
                    format!("STA (${:02X}),Y", vector_addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x94 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x(mem);
                let value = self.state.index_y;
                mem.write(addr, value);
                self.state.program_counter += 2;
                self.wait_cycles = 4;
                return Ok((
                    format!("STY ${:02X},X", base_addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0x95 => {
                let (base_addr, addr) = self.read_indexed_zeropage_x(mem);
                let value = self.state.accumulator;
                mem.write(addr, value);
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
                let abs_addr = self.read_absolute_addr(mem);
                let addr = abs_addr + self.state.index_y as u16;
                let value = self.state.accumulator;
                mem.write(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 5;
                return Ok((
                    format!("STA ${:04X},Y", abs_addr),
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
                let abs_addr = self.read_absolute_addr(mem);
                let addr = abs_addr + self.state.index_x as u16;
                let value = self.state.accumulator;
                mem.write(addr, value);
                self.state.program_counter += 3;
                self.wait_cycles = 5;
                return Ok((
                    format!("STA ${:04X},X", addr),
                    Some(Effect::WriteMem { addr, value })
                ));
            }
            0xA5 => {
                let addr = self.read_zeropage_addr(mem);
                let value = mem.read(addr);
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
                let value = self.read_immediate(mem);
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
                let value = mem.read(self.state.program_counter + 1);
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
                let addr = self.read_zeropage_addr(mem);
                let value = mem.read(addr);
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
                let addr = self.read_zeropage_addr(mem);
                let value = mem.read(addr);
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
                let value = mem.read(self.state.program_counter + 1);
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
                let addr = self.read_absolute_addr(mem);
                let value = mem.read(addr);
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
                let addr = self.read_absolute_addr(mem);
                let value = mem.read(addr);
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
                let addr = self.read_absolute_addr(mem);
                let value = mem.read(addr);
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
                let addr = self.read_relative_addr(mem) + 2;
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
                let (vector_addr, addr) = self.read_indirect_y_indexed_addr(mem);
                let value = mem.read(addr);
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
                let (base_addr, addr) = self.read_indexed_zeropage_x(mem);
                let value = mem.read(addr);
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
                let (base_addr, addr) = self.read_indexed_zeropage_x(mem);
                let value = mem.read(addr);
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
                let abs_addr = self.read_absolute_addr(mem);
                let addr = abs_addr + self.state.index_y as u16;
                let value = mem.read(addr);
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
            0xBA => {
                let value = self.state.stack_pointer;
                self.state.index_x = value;
                self.set_negative_flag(value);
                self.set_zero_flag(value);
                self.state.program_counter += 1;
                self.wait_cycles = 2;
                return Ok((
                    format!("TSX"),
                    None
                ));
            }
            0xBD => {
                let abs_addr = self.read_absolute_addr(mem);
                let addr = abs_addr + self.state.index_x as u16;
                let value = mem.read(addr);
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
            0xC0 => {
                let operand1 = self.state.index_y;
                let operand2 = self.read_immediate(mem);
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("CPY #${:02X}", operand2),
                    None
                ));
            }
            0xC4 => {
                let operand1 = self.state.index_y;
                let addr = self.read_zeropage_addr(mem);
                let operand2 = mem.read(addr);
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
                let addr = self.read_zeropage_addr(mem);
                let operand2 = mem.read(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("CMP ${:02X}", addr),
                    None
                ))
            }
            0xC6 => {
                let addr = self.read_zeropage_addr(mem);
                let effect = self.decrement_memory(mem, addr);
                self.state.program_counter += 2;
                self.wait_cycles = 6;
                return Ok((
                    format!("DEC ${:02X}", addr),
                    Some(effect)
                ));
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
                let operand2 = self.read_immediate(mem);
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
            0xCD => {
                let addr = self.read_absolute_addr(mem);
                let operand1 = self.state.accumulator;
                let operand2 = mem.read(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("CMP ${:04X}", addr),
                    None
                ));
            }
            0xD0 => {
                let addr = self.read_relative_addr(mem) + 2;
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
                let (vector_addr, addr) = self.read_indirect_y_indexed_addr(mem);
                let operand1 = self.state.accumulator;
                let operand2 = mem.read(addr);
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
                let abs_addr = self.read_absolute_addr(mem);
                let addr = abs_addr + self.state.index_x as u16;
                let operand1 = self.state.accumulator;
                let operand2 = mem.read(addr);
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
                let operand2 = self.read_immediate(mem);
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("CPX #${:02X}", operand2),
                    None
                ));
            }
            0xE4 => {
                let addr = self.read_zeropage_addr(mem);
                let operand1 = self.state.index_x;
                let operand2 = mem.read(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("CPX ${:02X}", addr),
                    None
                ));
            }
            0xE5 => {
                let addr = self.read_zeropage_addr(mem);
                let operand = mem.read(addr);
                self.subtract_with_carry(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 3;
                return Ok((
                    format!("SBC ${:02X}", addr),
                    None
                ));
            }
            0xE6 => {
                let addr = self.read_zeropage_addr(mem);
                let value = mem.read(addr) + 1;
                mem.write(addr, value);
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
            0xE9 => {
                let operand = self.read_immediate(mem);
                self.subtract_with_carry(operand);
                self.state.program_counter += 2;
                self.wait_cycles = 2;
                return Ok((
                    format!("SBC #${:02X}", operand),
                    None
                ));
            }
            0xEC => {
                let addr = self.read_absolute_addr(mem);
                let operand1 = self.state.index_x;
                let operand2 = mem.read(addr);
                self.compare(operand1, operand2);
                self.state.program_counter += 3;
                self.wait_cycles = 4;
                return Ok((
                    format!("CPX ${:04X}", addr),
                    None
                ));
            }
            0xF0 => {
                let addr = self.read_relative_addr(mem) + 2;
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
}