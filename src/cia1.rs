bitflags! {
    struct ICS: u8 {
        const TIMER_A_UNDERFLOW_INTERRUPT = 0b0000_0001;
        const TIMER_B_UNDERFLOW_INTERRUPT = 0b0000_0010;
        const TOD_ALARM_INTERRUPT         = 0b0000_0100;
        const SSR_RECV_SENT_INTERRUPT     = 0b0000_1000;
        const FLAG_PIN_POS_EDGE_INTERRUPT = 0b0001_0000;
    }
}

bitflags! {
    struct TACR: u8 {
        const START_TIMER                    = 0b0000_0001;
        const INDICATE_UNDERFLOW_ON_B6       = 0b0000_0010;
        const GEN_POS_EDGE_ON_B6_ON_UNDEFLOW = 0b0000_0100;
        const STOP_ON_UNDERFLOW              = 0b0000_1000;
        const LOAD_START_VALUE               = 0b0001_0000;
        const COUNT_CNT_POS_EDGES            = 0b0010_0000;
        const SSR_OUT                        = 0b0100_0000;
        const TOD_SPEED                      = 0b1000_0000;
    }
}

pub struct Cia1 {
    timer_a: u16,
    timer_a_start: u16,
    ics: ICS,
    tacr: TACR
}

pub enum Effect {
    IRQ
}

impl Cia1 {
    pub fn new() -> Cia1 {
        Cia1 {
            timer_a: 0,
            timer_a_start: 0,
            ics: ICS { bits: 0 },
            tacr: TACR { bits: 0 }
        }
    }

    pub fn write(self: &mut Cia1, addr: u16, value: u8) {
        match addr {
            0xDC04 => {
                self.timer_a_start = (self.timer_a_start & 0xFF00) | value as u16;
            }
            0xDC05 => {
                self.timer_a_start = (self.timer_a_start & 0x00FF) | ((value as u16) << 8);
            }
            0xDC0D => {
                self.ics.bits = value;
            }
            0xDC0E => {
                self.tacr.bits = value
            }
            _ => {
                println!("Unhandled write to CIA1: 0x{:02X} -> 0x{:04X}", value, addr);
            }
        }
    }

    pub fn read(self: &mut Cia1, addr: u16) -> u8 {
        match addr {
            0xDC04 => {
                (self.timer_a & 0x00FF) as u8
            }
            0xDC05 => {
                ((self.timer_a & 0xFF00) >> 8) as u8
            }
            0xDC0D => {
                let result = self.ics.bits;
                self.ics.bits = 0;
                result
            }
            0xDC0E => {
                self.tacr.bits
            }
            _ => {
                println!("Unhandled read from CIA1: 0x{:04X}", addr);
                0
            }
        }
    }

    fn timer_a_underflow(self: &mut Cia1) -> Option<Effect> {
        if self.tacr.contains(TACR::STOP_ON_UNDERFLOW) {
            self.tacr.set(TACR::START_TIMER, false);
        } else {
            self.timer_a = self.timer_a_start; // restart timer
        }
        let result = if self.ics.contains(ICS::TIMER_A_UNDERFLOW_INTERRUPT) {
            Some(Effect::IRQ)
        } else {
            None
        };
        self.ics.set(ICS::TIMER_A_UNDERFLOW_INTERRUPT, true);
        result
    }

    pub fn tick(self: &mut Cia1) -> Option<Effect> {
        if self.tacr.contains(TACR::START_TIMER) {
            match self.timer_a.checked_sub(1) {
                Some(result) => {
                    self.timer_a = result;
                    None
                }
                None => {
                    self.timer_a_underflow()
                }
            }
        } else {
            None
        }
    }
}