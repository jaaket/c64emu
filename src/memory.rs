pub trait ReadView {
    fn read(&self, addr: u16) -> u8;
}

pub trait WriteView {
    fn write(&mut self, addr: u16, value: u8) -> ();
}
