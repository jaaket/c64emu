#[macro_use] extern crate lazy_static;
extern crate regex;
extern crate rustyline;

use std::collections::HashSet;
use std::fs::File;
use std::io::prelude::*;

use regex::Regex;
use rustyline::error::ReadlineError;

mod memory;
use memory::{ReadView, WriteView};

mod mos6510;
use mos6510::Mos6510;
use mos6510::Effect;

mod vic_ii;
use vic_ii::VicII;

struct Machine {
    ram: [u8; 65536],
    io: [u8; 65536],
    char_rom: [u8; 4096],
    mos6510: Mos6510,
    vic_bank_start: u16,
    vic: VicII
}


#[derive(PartialEq)]
enum MemoryRegion {
    Rom,
    CharRom
}

struct Mos6510Memory<'a> {
    ram: &'a mut [u8],
    io: &'a mut [u8],
    vic_bank_start: u16,
    vic: &'a mut VicII
}

impl<'a> Mos6510Memory<'a> {
    fn new(ram: &'a mut [u8], io: &'a mut [u8], vic_bank_start: u16, vic: &'a mut VicII) -> Mos6510Memory<'a> {
        Mos6510Memory {
            ram,
            io,
            vic_bank_start,
            vic
        }
    }
}

impl<'a> ReadView for Mos6510Memory<'a> {
    fn read(self: &Mos6510Memory<'a>, addr: u16) -> u8 {
         if addr >= 0xD000 && addr < 0xD400 {
             // TODO: Read VIC-II registers
            self.io[addr as usize]
        } else if addr >= 0xD400 && addr < 0xE000 {
            self.io[addr as usize]
        } else {
            self.ram[addr as usize]
        }
    }
}

impl<'a> WriteView for Mos6510Memory<'a> {
    fn write(self: &mut Mos6510Memory<'a>, addr: u16, value: u8) -> () {
        // TODO: implement bank switching
        if (addr >= 0xA000 && addr < 0xC000) || addr >= 0xE000 {
            println!("Tried to write 0x{:02X} to ROM at 0x{:04X}, ignoring", value, addr);
        } else if addr >= 0xD000 && addr < 0xD400 {
            // This did nothing before refactoring also
            // self.vic.write(addr, value);
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
            self.ram[addr as usize] = value;
        }
    }
}

struct VicMemory<'a> {
    ram: &'a [u8]
}

impl<'a> VicMemory<'a> {
    fn new(ram: &'a [u8]) -> VicMemory {
        VicMemory {
            ram
        }
    }
}

impl<'a> ReadView for VicMemory<'a> {
    fn read(self: &VicMemory<'a>, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
}

impl Machine {
    fn new() -> Machine {
        Machine {
            ram: [0; 65536],
            io: [0; 65536],
            char_rom: [0; 4096],
            mos6510: Mos6510::new(),
            vic_bank_start: 0xC000,
            vic: VicII::new()
        }
    }

    fn reset(self: &mut Machine) {
        self.mos6510.reset(&Mos6510Memory::new(&mut self.ram, &mut self.io, self.vic_bank_start, &mut self.vic));
    }

    fn load_file(self: &mut Machine, filename: &str, memory_region: MemoryRegion, offset: usize) {
        {
            let f = File::open(filename).expect(&format!("file not found: {}", filename));
            let target =
                match memory_region {
                    MemoryRegion::Rom => &mut self.ram[offset..],
                    MemoryRegion::CharRom => &mut self.char_rom[offset..]
                };
            f.bytes().zip(target).for_each(|(byte, memory_byte)| *memory_byte = byte.unwrap());
        }

        if memory_region == MemoryRegion::CharRom {
            self.vic.char_rom.copy_from_slice(&self.char_rom[..]);
        }
    }

    fn tick(self: &mut Machine) -> Result<(Option<String>, Option<Effect>), String> {
        self.vic.tick(&VicMemory::new(&self.ram));
        self.mos6510.tick(&mut Mos6510Memory::new(&mut self.ram, &mut self.io, self.vic_bank_start, &mut self.vic))
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

    machine.load_file("basic.rom", MemoryRegion::Rom, 0xA000);
    machine.load_file("kernal.rom", MemoryRegion::Rom, 0xE000);
    machine.load_file("char.rom", MemoryRegion::CharRom, 0);

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
                machine.mos6510.print_status();
            }
            DebuggerState::Step => {
                println!();
                machine.mos6510.print_status();
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
                        machine.mos6510.print_status();
                    }
                    if debugger.breakpoints.contains(&machine.mos6510.get_pc()) {
                        debugger.state = DebuggerState::Pause;
                        println!("Breakpoint at 0x{:04X} reached", machine.mos6510.get_pc());
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
                let mem = Mos6510Memory::new(&mut machine.ram, &mut machine.io, machine.vic_bank_start, &mut machine.vic);
                println!("Memory at 0x{:04X}: 0x{:02X}", addr, mem.read(addr));
                debugger.state = DebuggerState::Pause;
            }
            DebuggerCommand::Exit => {
                break;
            }
        }
    }

    rl.save_history(history_path).unwrap();
}
