extern crate sdl2;
extern crate gl;

pub struct VicII {
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    event_pump: sdl2::EventPump,
    pub char_rom: [u8; 4096],
    char_rom_enabled: bool,
    raster_line: u16,
    x_coord: u16
}

fn find_sdl_gl_driver() -> Option<u32> {
    for (index, item) in sdl2::render::drivers().enumerate() {
        if item.name == "opengl" {
            return Some(index as u32);
        }
    }
    None
}

impl VicII {
    pub fn new() -> VicII {
        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();
        let window = video_subsystem.window("Window", 504, 312)
            .opengl()
            .build()
            .unwrap();
        let mut canvas = window.into_canvas()
            .index(find_sdl_gl_driver().unwrap())
            .build()
            .unwrap();

        let event_pump = sdl_context.event_pump().unwrap();

        canvas.set_draw_color(sdl2::pixels::Color::RGB(0, 0, 0));
        canvas.clear();
        canvas.present();
        canvas.set_draw_color(sdl2::pixels::Color::RGB(255, 255, 255));

        VicII {
            canvas: canvas,
            event_pump: event_pump,
            raster_line: 0,
            x_coord: 0,
            char_rom: [0; 4096],
            char_rom_enabled: false
        }
    }

    pub fn write(self: &mut VicII, addr: u16, value: u8) {
        // panic!("Unhandled write to VIC-II register: 0x{:02X} -> 0x{:04X}", value, addr);
    }

    pub fn read(self: &VicII, addr: u16, ram: &[u8]) -> u8 {
        if self.char_rom_enabled && addr >= 0x1000 && addr < 0x2000 {
            self.char_rom[addr as usize - 0x1000]
        } else {
            ram[addr as usize]
        }
    }

    pub fn enable_char_rom(self: &mut VicII) {
        self.char_rom_enabled = true;
    }

    pub fn disable_char_rom(self: &mut VicII) {
        self.char_rom_enabled = false;
    }

    fn first_line(self: &VicII) -> u16 {
        // TODO: Choose according to RSEL
        51
    }

    fn last_line(self: &VicII) -> u16 {
        // TODO: Choose according to RSEL
        250
    }

    fn first_x_coord(self: &VicII) -> u16 {
        // TODO: Choose according to CSEL
        24
    }

    fn last_x_coord(self: &VicII) -> u16 {
        // TODO: Choose according to CSEL
        343
    }

    pub fn tick(self: &mut VicII, ram: &[u8]) {
        if self.raster_line >= self.first_line() && self.raster_line <= self.last_line() &&
            self.x_coord >= self.first_x_coord() && self.x_coord <= self.last_x_coord() {

            let base_addr = 0x0400;
            let char_y = (self.raster_line - self.first_line()) / 8;
            let char_x = (self.x_coord - self.first_x_coord()) / 8;
            let char_addr = base_addr + char_y * 40 + char_x;
            let char_ptr = self.read(char_addr, ram) as u16;
            let data = self.read(0x1000 + char_ptr * 8 + (self.raster_line - self.first_line()) % 8, ram);

            for i in 0..8 {
                if data & (0x80 >> i) > 0 {
                    self.canvas.set_draw_color(sdl2::pixels::Color::RGB(255, 255, 255));
                } else {
                    self.canvas.set_draw_color(sdl2::pixels::Color::RGB(0, 0, 0));
                }
                self.canvas.draw_point((self.x_coord as i32 + i, self.raster_line as i32)).unwrap();
            }
        }

        self.canvas.present();

        self.x_coord += 8;
        if self.x_coord >= 504 {
            self.raster_line += 1;
            self.x_coord = 0;
        }
        if self.raster_line >= 312 {
            self.raster_line = 0
        }

        for event in self.event_pump.poll_iter() {
            use vic_ii::sdl2::event::Event;
            use vic_ii::sdl2::keyboard::Keycode;
            match event {
                Event::Quit {..} | Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    panic!("exit");
                }
                _ => ()
            }
        }
    }
}
