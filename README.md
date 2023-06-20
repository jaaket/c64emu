# C64EMU

A Commodore 64 emulator written in Rust.

Features for the MOS6510, VIC-II and CIA chips are implemented to the point where
the C64 boot screen appears and the cursor is blinking.

## Dependencies

* Rust toolchain (https://www.rust-lang.org/)
* SDL (https://www.libsdl.org/)

## Development

Download necessary Commodore 64 firmware files from http://www.zimmers.net/anonftp/pub/cbm/firmware/computers/c64/.
Three files are required:
* kernal ROM file
    * Download http://www.zimmers.net/anonftp/pub/cbm/firmware/computers/c64/kernal.901227-03.bin
    * Move the file to the project directory
    * Rename it as `kernal.rom`
* basic ROM file
    * Download http://www.zimmers.net/anonftp/pub/cbm/firmware/computers/c64/basic.901226-01.bin
    * Move the file to the project directory
    * Rename it as `basic.rom`
* character ROM file
    * Download http://www.zimmers.net/anonftp/pub/cbm/firmware/computers/c64/characters.325018-02.bin
    * Move the file to the project directory
    * Rename it as `char.rom`

To run the project:
```
cargo run
```

A debugger console appears displaying various data about the state of the emulator (see the `print_status` function in `mos6510.rs` for details):
```
pc      sp    n v - b d i z c  a     x     y     w
0xFCE2  0x00  0 0 - 0 0 0 0 0  0x00  0x00  0x00  0
>
```
Write `r` to the command prompt and press enter to run the emulator. The emulator isn't particularly fast, so it takes a while for the boot screen to appear.

Before running the emulator, it is possible to set up breakpoints and watchpoints using the commands:
* `b XXXX` sets a breakpoint at hexadecimal address `XXXX`
* `w XXXX` sets a watchpoint at hexadecimal address `XXXX`

At the moment breakpoints and watchpoints cannot be removed.

To exit the prompt, enter `CTRL+D`.