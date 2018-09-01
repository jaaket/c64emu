pub trait ReadView {
    fn read(&mut self, addr: u16) -> u8;
}

pub trait WriteView {
    fn write(&mut self, addr: u16, value: u8) -> ();
}
