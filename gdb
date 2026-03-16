set architecture riscv:rv32
target remote :1234
symbol-file ~/Documents/rust-baremetal/rust-baremetal/target/riscv32ima-unknown-none-elf/debug/main
dashboard -layout variables stack
layout asm
layout split 
break main::main
c
