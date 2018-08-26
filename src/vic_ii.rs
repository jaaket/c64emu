extern crate sdl2;
extern crate gl;

use memory::ReadView;

pub struct Registers {
    data: [u8; 47]
}

impl Registers {
    fn new() -> Registers {
        Registers {
            data: [0; 47]
        }
    }

    pub fn write(self: &mut Registers, addr: u16, value: u8) {
        self.data[(addr - 0xD000) as usize] = value;
        println!("Write to VIC register: ${:02X} -> ${:04X}", value, addr);
    }

    fn border_color(self: &Registers) -> u8 {
        self.data[0x20] & 0x0F
    }
}

const PALETTE: [sdl2::pixels::Color; 16] = [
    sdl2::pixels::Color { r: 0x00, g: 0x00, b: 0x00, a: 0x00 },
    sdl2::pixels::Color { r: 0xff, g: 0xff, b: 0xff, a: 0x00 },
    sdl2::pixels::Color { r: 0x81, g: 0x33, b: 0x38, a: 0x00 },
    sdl2::pixels::Color { r: 0x75, g: 0xce, b: 0xc8, a: 0x00 },
    sdl2::pixels::Color { r: 0x8e, g: 0x3c, b: 0x97, a: 0x00 },
    sdl2::pixels::Color { r: 0x56, g: 0xac, b: 0x4d, a: 0x00 },
    sdl2::pixels::Color { r: 0x2e, g: 0x2c, b: 0x9b, a: 0x00 },
    sdl2::pixels::Color { r: 0xed, g: 0xf1, b: 0x71, a: 0x00 },
    sdl2::pixels::Color { r: 0x8e, g: 0x50, b: 0x29, a: 0x00 },
    sdl2::pixels::Color { r: 0x55, g: 0x38, b: 0x00, a: 0x00 },
    sdl2::pixels::Color { r: 0xc4, g: 0x6c, b: 0x71, a: 0x00 },
    sdl2::pixels::Color { r: 0x4a, g: 0x4a, b: 0x4a, a: 0x00 },
    sdl2::pixels::Color { r: 0x7b, g: 0x7b, b: 0x7b, a: 0x00 },
    sdl2::pixels::Color { r: 0xa9, g: 0xff, b: 0x9f, a: 0x00 },
    sdl2::pixels::Color { r: 0x70, g: 0x6d, b: 0xeb, a: 0x00 },
    sdl2::pixels::Color { r: 0xb2, g: 0xb2, b: 0xb2, a: 0x00 }
];

pub struct VicII {
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    event_pump: sdl2::EventPump,
    raster_line: u16,
    x_coord: u16,
    pub registers: Registers
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
            registers: Registers::new()
        }
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
        96
    }

    fn last_x_coord(self: &VicII) -> u16 {
        // TODO: Choose according to CSEL
        415
    }

    pub fn tick<M: ReadView>(self: &mut VicII, mem: &M) {
        if self.raster_line >= self.first_line() && self.raster_line <= self.last_line() &&
            self.x_coord >= self.first_x_coord() && self.x_coord <= self.last_x_coord() {

            let base_addr = 0x0400;
            let char_y = (self.raster_line - self.first_line()) / 8;
            let char_x = (self.x_coord - self.first_x_coord()) / 8;
            let char_addr = base_addr + char_y * 40 + char_x;
            let char_ptr = mem.read(char_addr) as u16;
            let data = mem.read(0x1000 + char_ptr * 8 + (self.raster_line - self.first_line()) % 8);

            for i in 0..8 {
                if data & (0x80 >> i) > 0 {
                    self.canvas.set_draw_color(sdl2::pixels::Color::RGB(255, 255, 255));
                } else {
                    self.canvas.set_draw_color(sdl2::pixels::Color::RGB(0, 0, 0));
                }
                self.canvas.draw_point((self.x_coord as i32 + i, self.raster_line as i32)).unwrap();
            }
        }

        if (self.raster_line >= 0x08 && self.raster_line < self.first_line()) ||
            (self.raster_line > self.last_line() && self.raster_line <= 0x12C) ||
            (self.x_coord >= 52 && self.x_coord < self.first_x_coord()) ||
            (self.x_coord > self.last_x_coord() && self.x_coord <= 454) {

            self.canvas.set_draw_color(PALETTE[self.registers.border_color() as usize]);
            for i in 0..8 {
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
